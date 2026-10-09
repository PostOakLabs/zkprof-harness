// SPDX-License-Identifier: Apache-2.0

//! The `wgpu` wiring, and the only module here that needs a device.
//!
//! # Why there is no logic in `bindings.rs`
//!
//! There is none here either, beyond the sequence of calls. Everything that
//! can be decided without a device lives in [`crate::spec`], [`crate::m31`],
//! [`crate::run`] and [`crate::adapter`] and is tested natively, because this
//! module is `wasm32`-only and a test cannot reach it. See the crate docs.
//!
//! # Why the measured region starts after the profiler exists
//!
//! The profiler is constructed with the `Target`, and the target names the
//! adapter — which is not knowable until the adapter has been acquired. So
//! device acquisition is timed with the raw clock, and the profiler is built
//! afterwards with that duration handed in through
//! [`zkprof_core::Profiler::record`].
//!
//! That ordering also keeps the profiler's own clock-resolution probe out of
//! the reported region. The probe is instrumentation, not plumbing: it takes
//! thousands of clock readings to find the smallest step the clock can see,
//! and counting that as part of "how long does WebGPU take to start" would
//! be measuring the profiler. This is the same rule the rest of the
//! repository already follows — [`zkprof_core::Profiler::new`] probes before
//! it opens its region — applied where the probe cannot be first.

use std::borrow::Cow;
use std::sync::{Arc, Mutex, PoisonError};

use zkprof_core::browser::browser_target;
use zkprof_core::profiler::Profiler;
use zkprof_core::Label;

use crate::adapter::{adapter_string, webgpu_target};
use crate::completion::Gate;
use crate::m31;
use crate::run::{RunResult, COMPILE, DEVICE_ACQUIRE, DISPATCH, PRIMITIVE, READBACK, UPLOAD};
use crate::shader::{BINDING_LHS, BINDING_OUT, BINDING_RHS, BIND_GROUP, ENTRY_POINT, M31_MUL_WGSL};
use crate::spec::{self, elements_to_le_bytes, system, Spec};
use crate::timing::{
    ns_to_ms, read_timestamps, timestamp_delta_ns, DispatchTiming, GPU_PASS_PHASE, RESOLVED_BYTES,
    TIMESTAMP_SLOTS,
};
use crate::Error;

/// Runs one plumbing check and returns the envelope the page renders.
///
/// # Errors
///
/// Returns an [`Error`] for a spec that cannot be run, a platform with no
/// WebGPU adapter, a device or shader failure, a readback that came back the
/// wrong shape, or a device result that disagrees with the CPU reference.
/// The last of those is the one that matters most and is checked before any
/// report is allowed to exist.
pub async fn run(spec: Spec) -> Result<RunResult, Error> {
    spec.validate()?;

    let clock = zkprof_core::platform_clock()?;

    // --- device-acquire -------------------------------------------------
    // Timed with the raw clock, because the profiler cannot be built until
    // the adapter is known and the target can be filled in.
    let started_us = clock.now_us();
    let (device, queue, adapter_name) = acquire_device().await?;
    let acquire_us = clock.now_us() - started_us;

    let profiler = Profiler::new(
        clock.as_ref(),
        webgpu_target(browser_target(), &adapter_name),
        system(),
        spec.workload_id(),
    )
    .with_iterations(spec.dispatches);

    // The region opens here: after the resolution probe, before upload.
    let region_started_us = clock.now_us();
    profiler.record(Label::Phase(DEVICE_ACQUIRE.to_owned()), acquire_us);

    let (lhs, rhs) = spec.inputs();
    let expected = m31::mul_all(&lhs, &rhs);
    let bytes = spec.buffer_bytes();

    // --- upload ---------------------------------------------------------
    let started_us = clock.now_us();
    let (lhs_buffer, rhs_buffer, out_buffer, staging_buffer) = create_buffers(&device, bytes);
    queue.write_buffer(&lhs_buffer, 0, &elements_to_le_bytes(&lhs));
    queue.write_buffer(&rhs_buffer, 0, &elements_to_le_bytes(&rhs));
    profiler.record(Label::Phase(UPLOAD.to_owned()), clock.now_us() - started_us);

    // --- compile --------------------------------------------------------
    // Shader compilation and pipeline creation both report failures
    // asynchronously, because neither is synchronous on the WebGPU backend.
    // An error scope is the only way to see them at all; without it a typo in
    // the kernel produces a console message and a dispatch that does nothing.
    let started_us = clock.now_us();
    let (pipeline, bind_group) =
        build_pipeline(&device, &lhs_buffer, &rhs_buffer, &out_buffer).await?;
    profiler.record(
        Label::Phase(COMPILE.to_owned()),
        clock.now_us() - started_us,
    );

    // The instrument is built before the dispatch window opens, so its
    // creation is not charged to either figure. It is still real host time
    // and it still lands in the region; what it does not do is inflate the
    // wall-clock dispatch number by the cost of the query set that number is
    // about to be compared against.
    let timer = GpuTimer::new(&device, &queue);

    // --- dispatch -------------------------------------------------------
    // One submission, `spec.dispatches` kernel launches inside it. A single
    // submission is what lets the window below be one measurement rather
    // than a sum of N separately-latency-dominated ones.
    let started_us = clock.now_us();
    {
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        {
            let descriptor = wgpu::ComputePassDescriptor {
                label: Some("m31_mul"),
                // `None` when the device has no timestamp queries, which is
                // the W3.5 behaviour exactly: an unadorned compute pass.
                timestamp_writes: timer.as_ref().map(GpuTimer::timestamp_writes),
            };
            let mut pass = encoder.begin_compute_pass(&descriptor);
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(BIND_GROUP, &bind_group, &[]);
            for _ in 0..spec.dispatches {
                pass.dispatch_workgroups(spec.workgroups(), 1, 1);
            }
        }
        encoder.copy_buffer_to_buffer(&out_buffer, 0, &staging_buffer, 0, bytes);
        if let Some(timer) = &timer {
            timer.resolve(&mut encoder);
        }
        queue.submit(std::iter::once(encoder.finish()));
    }

    // Submission returns immediately; the device has not necessarily started.
    // Waiting for it to report completion is the only thing that makes this
    // window a statement about the device rather than about the host.
    let (gate, sender) = Gate::new();
    queue.on_submitted_work_done(move || sender.fire());
    gate.await;

    // Reading the instrument's own result stays inside the dispatch window.
    // Charge where it falls: it is 16 bytes, it is a cost of having the
    // instrument at all, and it is one more thing the pass figure cannot see
    // -- which is the entire reason both numbers ship.
    let gpu_pass_ms = match &timer {
        Some(timer) => timer.read().await?,
        None => None,
    };

    let dispatch_us = clock.now_us() - started_us;
    let timing = match gpu_pass_ms {
        Some(pass_ms) => {
            DispatchTiming::wall_clock_only(dispatch_us / 1_000.0).with_gpu_pass_ms(pass_ms)
        }
        None => DispatchTiming::wall_clock_only(dispatch_us / 1_000.0),
    };

    profiler.record(Label::Phase(DISPATCH.to_owned()), dispatch_us);
    // Recorded second so it reads next to `dispatch` in the report. The two
    // are the same region measured two ways, and a reader should not have to
    // hunt for the other one.
    if let Some(pass_ms) = timing.gpu_pass_ms {
        profiler.record(Label::Phase(GPU_PASS_PHASE.to_owned()), pass_ms * 1_000.0);
    }
    // The device figure when there is one. It is the narrower measurement:
    // the wall clock counts queue and submit overhead as though it were
    // field arithmetic, which is the same mislabelling this crate refuses to
    // commit with device acquisition.
    profiler.record(PRIMITIVE, timing.attributed_ms() * 1_000.0);

    // --- readback -------------------------------------------------------
    let started_us = clock.now_us();
    let produced = read_back(&staging_buffer).await?;
    profiler.record(
        Label::Phase(READBACK.to_owned()),
        clock.now_us() - started_us,
    );

    // --- verify before reporting ----------------------------------------
    check(&produced, &expected)?;

    // --- report ---------------------------------------------------------
    // Device acquisition was timed before the profiler existed, so it is
    // added back into the region the profiler cannot see for itself.
    profiler.set_region_ms((clock.now_us() - region_started_us + acquire_us) / 1_000.0);
    let mut report = profiler.finish()?;

    // Stamped after `finish`, because whether the instrument was available is
    // not known until the run has happened. Re-validated for the same reason:
    // `finish` validated a report without this key, and the document that
    // leaves the page is a different one.
    report.gpu_timing_source = Some(timing.source());
    report.validate()?;

    Ok(RunResult::new(report, adapter_name, spec, timing))
}

/// Acquires an adapter and a device, and names the adapter for the report.
///
/// Split out of [`run`] so each phase of the sequence reads as one step. The
/// acquisition is still timed by the caller, which owns the clock that has to
/// span it.
async fn acquire_device() -> Result<(wgpu::Device, wgpu::Queue, String), Error> {
    // `InstanceDescriptor` takes no `Default` in `wgpu` 30 -- it carries a
    // `display` handle that has no sensible default -- so the constructor is
    // used and only the field this crate actually cares about is overridden.
    // Spelling the rest out would break the build on the next `wgpu` release
    // that adds a field; `..` here cannot.
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::BROWSER_WEBGPU,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    // `request_adapter` returns a `Result`, not an `Option`: `wgpu` 22 split
    // "no adapter found" out from "the request was malformed". Both mean the
    // same thing to a caller that has nothing to retry with.
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions::default())
        .await
        .map_err(|_| Error::NoAdapter)?;

    let info = adapter.get_info();
    let adapter_name = adapter_string(
        &info.name,
        &format!("{:?}", info.backend),
        &format!("{:?}", info.device_type),
    );

    // Feature-detect before requesting. `required_features` is not a hint:
    // naming a feature the adapter does not advertise fails the *device*
    // request outright, which would turn "this machine has no timestamp
    // queries" into "this machine has no WebGPU". Asking for nothing and
    // checking what came back keeps the two failures distinct.
    let required_features = if adapter.features().contains(wgpu::Features::TIMESTAMP_QUERY) {
        wgpu::Features::TIMESTAMP_QUERY
    } else {
        wgpu::Features::empty()
    };

    let (device, queue) = adapter
        .request_device(&wgpu::DeviceDescriptor {
            required_features,
            ..wgpu::DeviceDescriptor::default()
        })
        .await?;
    Ok((device, queue, adapter_name))
}

/// The timestamp-query instrument, when the device has the feature.
///
/// Held together because the three objects are only ever used as a unit: the
/// query set is written by the pass, resolved into a buffer the device can
/// write, and copied into one the host can map. `QUERY_RESOLVE` and
/// `MAP_READ` cannot share a buffer — `MAP_READ` is only legal alongside
/// `COPY_DST` — so the resolve buffer and the readback buffer are two
/// buffers and the copy between them is one command.
struct GpuTimer {
    query_set: wgpu::QuerySet,
    resolve_buffer: wgpu::Buffer,
    readback_buffer: wgpu::Buffer,
    period_ns: f32,
}

impl GpuTimer {
    /// Builds the instrument, or returns `None` on a device without it.
    ///
    /// Detecting on the *device* rather than the adapter is deliberate. The
    /// adapter advertises what it can do; `required_features` is what was
    /// asked for; `device.features()` is what this device actually has. The
    /// last of those is the only one the pass may use.
    fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Option<Self> {
        if !device.features().contains(wgpu::Features::TIMESTAMP_QUERY) {
            return None;
        }
        let query_set = device.create_query_set(&wgpu::QuerySetDescriptor {
            label: Some("m31 pass timestamps"),
            ty: wgpu::QueryType::Timestamp,
            count: TIMESTAMP_SLOTS,
        });
        let resolve_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("m31 timestamps resolve"),
            size: RESOLVED_BYTES,
            usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let readback_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("m31 timestamps readback"),
            size: RESOLVED_BYTES,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Some(Self {
            query_set,
            resolve_buffer,
            readback_buffer,
            // One timestamp tick is this many nanoseconds. The browser
            // reports 1.0; native backends have reported coarser values, and
            // hardcoding 1.0 would silently rescale every figure.
            period_ns: queue.get_timestamp_period(),
        })
    }

    /// The descriptor fragment that brackets the pass.
    ///
    /// `timestamp_writes` and not [`wgpu::ComputePass::write_timestamp`]:
    /// writing a timestamp inside a pass was removed from the WebGPU
    /// specification (gpuweb#4370), and `wgpu`'s browser backend *panics* on
    /// the calls it kept for native backends. The descriptor form is the only
    /// one the browser implements, so it is the only one used here.
    fn timestamp_writes(&self) -> wgpu::ComputePassTimestampWrites<'_> {
        wgpu::ComputePassTimestampWrites {
            query_set: &self.query_set,
            beginning_of_pass_write_index: Some(0),
            end_of_pass_write_index: Some(1),
        }
    }

    /// Resolves the two timestamps into the buffer the host can map.
    ///
    /// Outside the pass, necessarily: a query cannot be resolved while the
    /// pass that writes it is open.
    fn resolve(&self, encoder: &mut wgpu::CommandEncoder) {
        encoder.resolve_query_set(&self.query_set, 0..TIMESTAMP_SLOTS, &self.resolve_buffer, 0);
        encoder.copy_buffer_to_buffer(
            &self.resolve_buffer,
            0,
            &self.readback_buffer,
            0,
            RESOLVED_BYTES,
        );
    }

    /// Reads the resolved pair back as a duration in milliseconds.
    ///
    /// `Ok(None)` means the readback did not describe a duration — the query
    /// was never written, or the period was unusable. That is not an error:
    /// a machine can advertise the feature and still hand back a pair this
    /// code refuses to publish, and the correct response is to fall back to
    /// the wall clock and say so, not to fail the run.
    async fn read(&self) -> Result<Option<f64>, Error> {
        let bytes = map_read(&self.readback_buffer).await?;
        let Some((begin, end)) = read_timestamps(&bytes) else {
            return Ok(None);
        };
        Ok(timestamp_delta_ns(begin, end, self.period_ns).map(ns_to_ms))
    }
}

/// Creates the two operand buffers, the output buffer and the staging buffer.
///
/// All four are allocated before any of them is written, so the upload phase
/// measures the writes rather than the allocations behind them.
fn create_buffers(
    device: &wgpu::Device,
    bytes: u64,
) -> (wgpu::Buffer, wgpu::Buffer, wgpu::Buffer, wgpu::Buffer) {
    let lhs_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("m31 lhs"),
        size: bytes,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let rhs_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("m31 rhs"),
        size: bytes,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let out_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("m31 out"),
        size: bytes,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let staging_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("m31 staging"),
        size: bytes,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    (lhs_buffer, rhs_buffer, out_buffer, staging_buffer)
}

/// Compiles the kernel, then builds the pipeline and its bind group.
///
/// # Errors
///
/// Returns [`Error::Shader`] carrying the device's own diagnostic text if
/// compilation or pipeline creation was rejected.
async fn build_pipeline(
    device: &wgpu::Device,
    lhs_buffer: &wgpu::Buffer,
    rhs_buffer: &wgpu::Buffer,
    out_buffer: &wgpu::Buffer,
) -> Result<(wgpu::ComputePipeline, wgpu::BindGroup), Error> {
    // In `wgpu` 30 the scope is an RAII guard rather than a bare push: `pop`
    // consumes it, and dropping it un-popped would silently leak the scope.
    let error_scope = device.push_error_scope(wgpu::ErrorFilter::Validation);

    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("m31_mul"),
        source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(M31_MUL_WGSL)),
    });
    let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("m31_mul"),
        entries: &[
            storage_entry(BINDING_LHS, true),
            storage_entry(BINDING_RHS, true),
            storage_entry(BINDING_OUT, false),
        ],
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("m31_mul"),
        bind_group_layouts: &[Some(&bind_group_layout)],
        // `wgpu` 30 replaced `push_constant_ranges` with a byte count. The
        // kernel takes no immediates, so nothing is allocated.
        immediate_size: 0,
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("m31_mul"),
        layout: Some(&pipeline_layout),
        module: &module,
        entry_point: Some(ENTRY_POINT),
        compilation_options: wgpu::PipelineCompilationOptions::default(),
        cache: None,
    });
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("m31_mul"),
        layout: &bind_group_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: BINDING_LHS,
                resource: lhs_buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: BINDING_RHS,
                resource: rhs_buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: BINDING_OUT,
                resource: out_buffer.as_entire_binding(),
            },
        ],
    });

    if let Some(error) = error_scope.pop().await {
        return Err(Error::Shader(format!("{error}")));
    }
    Ok((pipeline, bind_group))
}

/// Maps the staging buffer, copies the product out of it and unmaps it.
///
/// # Errors
///
/// Returns [`Error::Map`] if the mapping was rejected, [`Error::View`] if a
/// view over the mapped range was refused, and [`Error::RaggedReadback`] if
/// the byte count is not a whole number of elements.
async fn read_back(staging_buffer: &wgpu::Buffer) -> Result<Vec<u32>, Error> {
    let bytes = map_read(staging_buffer).await?;
    elements_to_le_bytes_check(&bytes)
}

/// Maps a buffer read-only, copies its bytes out and unmaps it.
///
/// # Errors
///
/// Returns [`Error::Map`] if the mapping was rejected and [`Error::View`] if a
/// view over the mapped range was refused.
async fn map_read(buffer: &wgpu::Buffer) -> Result<Vec<u8>, Error> {
    // `map_async` is callback-based in `wgpu` 30: it returns nothing
    // awaitable, so the same `Gate` that waits on submission waits on the
    // mapping here.
    //
    // The callback's `Result` is carried out rather than discarded. Dropping
    // it would be the easy version -- `get_mapped_range` also fails when the
    // map failed -- but it would report a view error for a mapping failure,
    // and a crate whose only job is to notice a plumbing failure before it is
    // published as a timing should say which stage broke.
    let (mapped, sender) = Gate::new();
    let outcome: Arc<Mutex<Option<Result<(), Error>>>> = Arc::new(Mutex::new(None));
    let sink = Arc::clone(&outcome);
    buffer
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            let mut slot = sink.lock().unwrap_or_else(PoisonError::into_inner);
            *slot = Some(result.map_err(Error::from));
            drop(slot);
            sender.fire();
        });
    mapped.await;
    outcome
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take()
        .unwrap_or(Ok(()))?;

    let bytes = {
        let view = buffer.slice(..).get_mapped_range()?;
        view.to_vec()
    };
    buffer.unmap();
    Ok(bytes)
}

/// Compares the device's output against the CPU reference.
///
/// # Errors
///
/// Returns [`Error::WrongElementCount`] if the counts differ, and
/// [`Error::Mismatch`] naming the first offending index if a value does.
fn check(produced: &[u32], expected: &[u32]) -> Result<(), Error> {
    if produced.len() != expected.len() {
        return Err(Error::WrongElementCount {
            elements: produced.len(),
            expected: expected.len(),
        });
    }
    for (index, (want, got)) in expected.iter().zip(produced.iter()).enumerate() {
        if want != got {
            return Err(Error::Mismatch {
                index,
                expected: *want,
                actual: *got,
            });
        }
    }
    Ok(())
}

/// One storage-buffer binding in the bind group layout.
const fn storage_entry(binding: u32, read_only: bool) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

/// Decodes the mapped staging buffer.
fn elements_to_le_bytes_check(bytes: &[u8]) -> Result<Vec<u32>, Error> {
    spec::elements_from_le_bytes(bytes).ok_or(Error::RaggedReadback { bytes: bytes.len() })
}
