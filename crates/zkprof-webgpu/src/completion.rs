// SPDX-License-Identifier: Apache-2.0

//! A one-shot callback turned into something `await`-able.
//!
//! # Why this exists
//!
//! `wgpu::Queue::on_submitted_work_done` takes a callback and returns
//! nothing. There is no future to await, so the moment a submission finishes
//! cannot simply be `await`ed — and that moment is the only thing separating
//! "the kernel ran" from "the buffer got mapped". Without it the dispatch
//! phase measures command encoding, which is host-side work measured in
//! microseconds and says nothing about the device.
//!
//! [`Gate`] is the smallest thing that bridges the two. It is ordinary Rust
//! with no `wgpu` in it, which is what lets the bridge be tested on a machine
//! with no GPU.
//!
//! # What it does not do
//!
//! It does not time out. If the device never reports the submission finished,
//! `Gate` never resolves and the promise the page awaits never settles. A
//! deadline is the right fix and needs a timer API this crate does not
//! otherwise touch; adding one here would mean shipping an untested failure
//! path, which is worse than documenting the limitation.

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::task::{Context, Poll, Waker};

/// The two ends of one submission's completion signal.
///
/// `Gate` is awaited; [`GateSender`] is handed to whatever will eventually
/// report completion. The sender is `Send` and `'static`, which is what
/// `wgpu`'s callback bound requires, and it holds no borrow of the gate — so
/// the gate can be dropped without the sender becoming unsound to call.
pub struct Gate {
    shared: Arc<Mutex<Shared>>,
}

/// The reporting end. Firing it wakes the awaiting [`Gate`].
pub struct GateSender {
    shared: Arc<Mutex<Shared>>,
}

#[derive(Default)]
struct Shared {
    done: bool,
    waker: Option<Waker>,
}

impl Gate {
    /// Creates a gate and the sender that fires it.
    #[must_use]
    pub fn new() -> (Self, GateSender) {
        let shared = Arc::new(Mutex::new(Shared::default()));
        (
            Self {
                shared: Arc::clone(&shared),
            },
            GateSender { shared },
        )
    }

    /// Whether completion has been reported.
    ///
    /// Exposed for tests and for a caller that wants to distinguish "not yet"
    /// from "not going to". It is not a substitute for awaiting.
    #[must_use]
    pub fn is_done(&self) -> bool {
        lock(&self.shared).done
    }
}

impl Future for Gate {
    type Output = ();

    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        let mut shared = lock(&self.shared);
        if shared.done {
            return Poll::Ready(());
        }
        shared.waker = Some(context.waker().clone());
        Poll::Pending
    }
}

impl GateSender {
    /// Reports completion, waking the awaiting gate if there is one.
    ///
    /// Idempotent: a second call changes nothing. `wgpu` runs the callback
    /// exactly once, but the method takes `&self` so the gate can outlive
    /// the thing that fired it.
    pub fn fire(&self) {
        let mut shared = lock(&self.shared);
        shared.done = true;
        if let Some(waker) = shared.waker.take() {
            waker.wake();
        }
    }
}

/// Locks `shared`, treating a poisoned mutex as still usable.
///
/// Nothing panics while this lock is held — the critical sections move a bool
/// and a waker — so poisoning cannot happen. Recovering anyway is the cheaper
/// assumption to be wrong about.
fn lock(shared: &Mutex<Shared>) -> MutexGuard<'_, Shared> {
    shared.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::Gate;
    use std::future::Future;
    use std::pin::Pin;
    use std::task::{Context, Poll, Waker};

    fn poll(gate: &mut Gate) -> Poll<()> {
        // `Waker::noop()` is the standard-library's own do-nothing waker; it
        // exists precisely so tests that poll a future by hand do not each
        // have to write one.
        let mut context = Context::from_waker(Waker::noop());
        Pin::new(gate).poll(&mut context)
    }

    #[test]
    fn a_fresh_gate_is_pending() {
        let (mut gate, _sender) = Gate::new();
        assert!(!gate.is_done());
        assert_eq!(poll(&mut gate), Poll::Pending);
    }

    #[test]
    fn firing_resolves_the_gate() {
        let (mut gate, sender) = Gate::new();
        assert_eq!(poll(&mut gate), Poll::Pending);

        sender.fire();

        assert!(gate.is_done());
        assert_eq!(poll(&mut gate), Poll::Ready(()));
    }

    #[test]
    fn firing_before_the_first_poll_is_not_lost() {
        // The wake-up has already happened by the time anyone polls. A gate
        // that only registered its waker on the first poll would hang here.
        let (mut gate, sender) = Gate::new();
        sender.fire();
        assert_eq!(poll(&mut gate), Poll::Ready(()));
    }

    #[test]
    fn firing_twice_is_harmless() {
        let (mut gate, sender) = Gate::new();
        sender.fire();
        sender.fire();
        assert!(gate.is_done());
        assert_eq!(poll(&mut gate), Poll::Ready(()));
    }

    #[test]
    fn the_gate_still_resolves_if_it_was_never_polled_first() {
        let (gate, sender) = Gate::new();
        assert!(!gate.is_done());
        sender.fire();
        assert!(gate.is_done());
    }

    #[test]
    fn a_dropped_gate_does_not_make_firing_fail() {
        // The sender is handed to a callback that outlives everything here,
        // so firing after the gate is gone has to be a no-op rather than a
        // panic.
        let (gate, sender) = Gate::new();
        drop(gate);
        sender.fire();
    }
}
