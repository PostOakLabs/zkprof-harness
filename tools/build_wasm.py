#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Build the browser modules in both wasm profiles and run wasm-bindgen on them.

    python tools/build_wasm.py                     # every module, scalar + simd
    python tools/build_wasm.py --only simd
    python tools/build_wasm.py --module marshal
    python tools/build_wasm.py --only threads      # plumbed, not implemented

# The two modules

`harness` is the general ZK-profiling page backed by `zkprof-wasm`.
`marshal` is the JS<->WASM boundary microbench backed by `zkprof-marshal`,
and it is deliberately a separate wasm module rather than a second set of
exports on the first: the microbench measures the cost of a crossing, so its
surface has to stay small enough that a reader can see every function it
contains and convince themselves none of them hide extra work. Merging it
into the general module would make that argument impossible to make.

# Why this script exists rather than a cargo alias

Two things have to happen in the right order with the right environment, and
neither is expressible in Cargo:

1. `RUSTFLAGS` differs per profile, and Cargo keys its build cache on it. A
   scalar build and a simd build are separate compilations, so they go to
   separate invocations -- not to one invocation with a flag that would
   invalidate the other's cache on every switch.
2. The wasm-bindgen **CLI** version must equal the wasm-bindgen **crate**
   version in `Cargo.lock`. The generated glue and the Rust side share a
   binary protocol; a mismatch produces a module that fails at instantiation
   with an error that names neither the tool nor the version. This script
   checks before it builds.

# Getting the CLI

`cargo install wasm-bindgen-cli` does not work everywhere: it needs `dlltool`
on Windows, which a MinGW-less Rust install does not ship. The release ships
prebuilt binaries, so this script points at one of those instead. It never
installs anything globally.

    curl -sSLO https://github.com/rustwasm/wasm-bindgen/releases/download/\
0.2.127/wasm-bindgen-0.2.127-x86_64-pc-windows-msvc.tar.gz
    curl -sSLO .../wasm-bindgen-0.2.127-x86_64-pc-windows-msvc.tar.gz.sha256sum
    sha256sum -c wasm-bindgen-0.2.127-x86_64-pc-windows-msvc.tar.gz.sha256sum
    tar -xzf wasm-bindgen-0.2.127-x86_64-pc-windows-msvc.tar.gz -C tools/bin/

The expected sha256 for the archive is printed by `--print-download-info`.
"""

from __future__ import annotations

import argparse
import os
import re
import shutil
import subprocess
import sys
import tomllib
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
CARGO_LOCK = REPO_ROOT / "Cargo.lock"
TOOLS_BIN = REPO_ROOT / "tools" / "bin"

TARGET = "wasm32-unknown-unknown"

# Each module is one crate plus the directory its generated packages land in.
# `profiles` is explicit because `threads` is only plumbed for the harness:
# the marshal crate has no threaded paths to gate yet.
MODULES: dict[str, dict[str, object]] = {
    "harness": {
        "package": "zkprof-wasm",
        "out_root": REPO_ROOT / "web" / "harness",
        "profiles": ["scalar", "simd", "threads"],
    },
    "marshal": {
        "package": "zkprof-marshal",
        "out_root": REPO_ROOT / "web" / "marshalling-bench",
        "profiles": ["scalar", "simd"],
    },
}

# The release asset this script expects. Kept here so the fetch above is
# reproducible and checkable rather than "grab whatever is newest".
EXPECTED_VERSION = "0.2.127"
WINDOWS_ASSET = f"wasm-bindgen-{EXPECTED_VERSION}-x86_64-pc-windows-msvc.tar.gz"
WINDOWS_ASSET_SHA256 = "fd52ef9896cbb0f4f59ef115bdf2f4664a76d716b6201c35e5936018ad680cfc"
MUSL_ASSET = f"wasm-bindgen-{EXPECTED_VERSION}-x86_64-unknown-linux-musl.tar.gz"
MUSL_ASSET_SHA256 = "61d4a7dc85acfa0d2354ccc0b8361928c7e52a746d17f28ebaa795ed3dc1614a"

PROFILES: dict[str, dict[str, object]] = {
    # `kind` is the `target.kind` the schema spells for this build; it is what
    # the compiled code reports, and it comes from cfg, not from here.
    "scalar": {"kind": "wasm-scalar", "rustflags": None, "features": []},
    "simd": {"kind": "wasm-simd", "rustflags": "-C target-feature=+simd128", "features": []},
    "threads": {
        "kind": "wasm-threads",
        "rustflags": None,
        "features": ["web-threads"],
        "unimplemented": True,
    },
}


class BuildError(RuntimeError):
    """Anything that stops the build, with a message worth printing alone."""


def locked_wasm_bindgen_version() -> str:
    """The wasm-bindgen version `Cargo.lock` resolved to."""
    if not CARGO_LOCK.is_file():
        raise BuildError(f"no Cargo.lock at {CARGO_LOCK}")
    document = tomllib.loads(CARGO_LOCK.read_text(encoding="utf-8"))
    for package in document.get("package", []):
        if package.get("name") == "wasm-bindgen":
            return str(package["version"])
    raise BuildError("Cargo.lock has no wasm-bindgen package; is it a workspace dependency?")


def find_cli() -> Path:
    """Locate the wasm-bindgen CLI.

    Order: `$WASM_BINDGEN`, anything extracted under `tools/bin/`, then `PATH`.
    A repo-local copy wins over PATH on purpose -- the version has to match
    `Cargo.lock`, and a machine-wide CLI is whatever version it happens to be.
    """
    override = os.environ.get("WASM_BINDGEN")
    if override:
        candidate = Path(override)
        if not candidate.is_file():
            raise BuildError(f"WASM_BINDGEN points at {candidate}, which is not a file")
        return candidate

    names = ["wasm-bindgen.exe", "wasm-bindgen"]
    if TOOLS_BIN.is_dir():
        found = sorted(
            path for path in TOOLS_BIN.rglob("*") if path.is_file() and path.name in names
        )
        if found:
            return found[0]

    on_path = shutil.which("wasm-bindgen")
    if on_path:
        return Path(on_path)

    raise BuildError(
        "could not find the wasm-bindgen CLI.\n"
        "Run `python tools/build_wasm.py --print-download-info` for the exact\n"
        "asset and checksum, or set WASM_BINDGEN to a binary you already have."
    )


def cli_version(cli: Path) -> str:
    """The version string the CLI reports, e.g. `wasm-bindgen 0.2.127`."""
    try:
        completed = subprocess.run(
            [str(cli), "--version"], capture_output=True, text=True, check=True
        )
    except OSError as error:
        raise BuildError(f"could not run {cli}: {error}") from error
    except subprocess.CalledProcessError as error:
        raise BuildError(f"{cli} --version exited {error.returncode}") from error

    match = re.search(r"(\d+\.\d+\.\d+)", completed.stdout)
    if match is None:
        raise BuildError(f"could not read a version out of {completed.stdout!r}")
    return match.group(1)


def run(command: list[str], env: dict[str, str] | None = None) -> None:
    """Run a command, echoing it first."""
    printable = " ".join(command)
    print(f"  $ {printable}", flush=True)
    try:
        subprocess.run(command, cwd=REPO_ROOT, env=env, check=True)
    except subprocess.CalledProcessError as error:
        raise BuildError(f"command failed ({error.returncode}): {printable}") from error


def build_profile(
    module: str, name: str, spec: dict[str, object], cli: Path, release: bool
) -> Path:
    """Compile one profile of one module and run wasm-bindgen over it."""
    kind = str(spec["kind"])
    package = str(MODULES[module]["package"])
    out_dir = Path(str(MODULES[module]["out_root"])) / f"pkg-{name}"
    print(f"\n=== {module}/{name} -> {kind} ===")

    env = dict(os.environ)
    rustflags = spec["rustflags"]
    if rustflags is None:
        env.pop("RUSTFLAGS", None)
    else:
        env["RUSTFLAGS"] = str(rustflags)

    cargo = [
        "cargo",
        "build",
        "--target",
        TARGET,
        "--package",
        package,
    ]
    if release:
        cargo.append("--release")
    for feature in spec["features"]:  # type: ignore[union-attr]
        cargo.extend(["--features", str(feature)])

    run(cargo, env)

    # Cargo puts the artifact under target/<triple>/<profile>/.
    profile_dir = "release" if release else "debug"
    built = REPO_ROOT / "target" / TARGET / profile_dir / f"{package.replace('-', '_')}.wasm"
    if not built.is_file():
        raise BuildError(f"expected {built} after a successful build, but it is missing")

    if spec.get("unimplemented"):
        print(
            f"  NOTE: the {name} profile is plumbed but not implemented. The module\n"
            f"        builds and reports target.kind '{kind}', and run_benchmark\n"
            f"        refuses every call. No report can come out of it."
        )

    out_dir.mkdir(parents=True, exist_ok=True)
    run(
        [
            str(cli),
            "--target",
            "web",
            "--out-dir",
            str(out_dir),
            "--no-typescript",
            str(built),
        ]
    )

    print(f"  -> {out_dir.relative_to(REPO_ROOT)}")
    return out_dir


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "--only",
        choices=sorted(PROFILES),
        help="build one profile instead of the default scalar + simd",
    )
    parser.add_argument(
        "--module",
        choices=sorted(MODULES),
        help="build one module instead of all of them",
    )
    parser.add_argument(
        "--debug",
        action="store_true",
        help="build without --release (much slower measurements, faster builds)",
    )
    parser.add_argument(
        "--print-download-info",
        action="store_true",
        help="print the prebuilt CLI asset and its checksum, then exit",
    )
    args = parser.parse_args()

    if args.print_download_info:
        print(f"version:  {EXPECTED_VERSION}")
        print(f"windows:  {WINDOWS_ASSET}")
        print(f"sha256:   {WINDOWS_ASSET_SHA256}")
        print(f"musl:     {MUSL_ASSET}")
        print(f"musl_sha256: {MUSL_ASSET_SHA256}")
        print(
            f"base url: https://github.com/rustwasm/wasm-bindgen/releases/download/{EXPECTED_VERSION}"
        )
        return 0

    try:
        locked = locked_wasm_bindgen_version()
        cli = find_cli()
        actual = cli_version(cli)
    except BuildError as error:
        print(f"error: {error}", file=sys.stderr)
        return 1

    print(f"Cargo.lock wants wasm-bindgen {locked}")
    print(f"CLI at {cli} reports {actual}")

    if locked != actual:
        print(
            f"\nerror: version mismatch.\n"
            f"  Cargo.lock resolved wasm-bindgen {locked}\n"
            f"  the CLI at {cli} is {actual}\n"
            f"\nThe generated glue and the Rust side share a binary protocol. "
            f"A mismatch\nproduces a module that fails at instantiation with an "
            f"error naming neither\nthe tool nor the version, so this is "
            f"refused here rather than left to surface in the\nbrowser console.\n"
            f"\nFetch {WINDOWS_ASSET} (see --print-download-info) or point "
            f"WASM_BINDGEN at a\nbinary of the matching version.",
            file=sys.stderr,
        )
        return 1

    if locked != EXPECTED_VERSION:
        print(
            f"warning: this script was written against {EXPECTED_VERSION}, but "
            f"Cargo.lock\n         resolves {locked}. The version check above "
            f"has passed; the checksum in\n         --print-download-info may "
            f"be stale.",
            file=sys.stderr,
        )

    selected = [args.only] if args.only else ["scalar", "simd"]
    modules = [args.module] if args.module else sorted(MODULES)

    try:
        for module in modules:
            supported = MODULES[module]["profiles"]
            for name in selected:
                if name not in supported:  # type: ignore[operator]
                    print(f"  skipping {module}/{name}: not a profile of that module")
                    continue
                build_profile(module, name, PROFILES[name], cli, release=not args.debug)
    except BuildError as error:
        print(f"\nerror: {error}", file=sys.stderr)
        return 1

    print(f"\nbuilt {', '.join(modules)} ({', '.join(selected)})")
    print("serve with: python web/server.py")
    return 0


if __name__ == "__main__":
    sys.exit(main())
