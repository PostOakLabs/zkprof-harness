#!/usr/bin/env python3
"""Verify the vendored report schema still matches what the crate claims.

`crates/zkprof-core/schema/zkprof-report.schema.json` is a copy of the file
owned by `PostOakLabs/engineering-decision-schema`. Copies drift. This script checks the two
things that can go wrong, using nothing but the standard library so it can run
anywhere a checkout exists:

1. The sha256 of the vendored file equals the `SCHEMA_SHA256` constant in
   `crates/zkprof-core/src/schema.rs`. The Rust constant is the source of
   truth: updating the vendored file means updating it there too.
2. The `$id` inside the vendored file equals the `SCHEMA_ID` constant, which is
   the value stamped into every report's `$schema` key.

Optional third check, off by default because it needs a sibling checkout:
`--against PATH` compares the vendored file byte-for-byte with another copy,
e.g. `../edi-schema/schemas/zkprof-report.schema.json`.

> **Why CI does not pass `--against` yet (deferral, 2026-09-02 / R4):** wiring
> `--against` into CI would make the drift gate compare against the thing it
> names, closing the SCHEMA-1 tautology (the bare check compares the vendored
> file only to `SCHEMA_SHA256`, a constant computed from that same drifted
> file). But the vendored copy is currently **ahead** of the upstream
> edi-schema `zkprof-report.schema.json` — it adds `backend_id`,
> `commitment_scheme` and `version`, fields the upstream schema does not have.
> Which direction to reconcile (keep the fields in the vendored copy, or add
> them upstream) is a schema-semantics decision owned by the maintainers, not by this
> script. Wiring `--against` before that decision lands would make every CI run permanently
> red on the known drift with no way to resolve it here. The mechanism is live
> and provably catches edits — `python tools/check_vendored_schema.py --against
> PATH` returns nonzero on any byte difference (verified 2026-09-02); the
> wiring is deferred until that decision rules the content.

Exit status is 0 when everything matches, 1 otherwise.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
VENDORED_SCHEMA = REPO_ROOT / "crates" / "zkprof-core" / "schema" / "zkprof-report.schema.json"
RUST_CONSTANTS = REPO_ROOT / "crates" / "zkprof-core" / "src" / "schema.rs"

# `pub const SCHEMA_SHA256: &str =` and the string literal may sit on different
# lines, so allow any whitespace between them.
CONSTANT_PATTERNS = {
    "SCHEMA_SHA256": re.compile(
        r'pub const SCHEMA_SHA256:\s*&str\s*=\s*"([0-9a-f]{64})"\s*;',
        re.DOTALL,
    ),
    "SCHEMA_ID": re.compile(r'pub const SCHEMA_ID:\s*&str\s*=\s*"([^"]+)"\s*;', re.DOTALL),
}


def sha256_of(path: Path) -> str:
    """Return the hex sha256 of a file, read in chunks."""
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(65_536), b""):
            digest.update(chunk)
    return digest.hexdigest()


def read_rust_constants(path: Path) -> dict[str, str]:
    """Extract the pinned constants from the Rust source.

    Raises `LookupError` if a constant is missing or malformed, which means the
    Rust file was edited in a way this script no longer understands.
    """
    source = path.read_text(encoding="utf-8")
    found: dict[str, str] = {}
    for name, pattern in CONSTANT_PATTERNS.items():
        match = pattern.search(source)
        if match is None:
            raise LookupError(f"could not find `pub const {name}` in {path}")
        found[name] = match.group(1)
    return found


def check(against: Path | None) -> list[str]:
    """Run every check and return a list of human-readable failures."""
    failures: list[str] = []

    if not VENDORED_SCHEMA.is_file():
        return [f"vendored schema is missing: {VENDORED_SCHEMA}"]
    if not RUST_CONSTANTS.is_file():
        return [f"Rust constants file is missing: {RUST_CONSTANTS}"]

    try:
        constants = read_rust_constants(RUST_CONSTANTS)
    except LookupError as error:
        return [str(error)]

    try:
        document = json.loads(VENDORED_SCHEMA.read_text(encoding="utf-8"))
    except json.JSONDecodeError as error:
        return [f"vendored schema is not valid JSON: {error}"]

    actual_sha256 = sha256_of(VENDORED_SCHEMA)
    if actual_sha256 != constants["SCHEMA_SHA256"]:
        failures.append(
            "vendored schema sha256 does not match SCHEMA_SHA256\n"
            f"    file:     {actual_sha256}\n"
            f"    constant: {constants['SCHEMA_SHA256']}\n"
            "    Update both together: the constant is the source of truth."
        )

    declared_id = document.get("$id")
    if declared_id != constants["SCHEMA_ID"]:
        failures.append(
            "schema $id does not match SCHEMA_ID\n"
            f"    file:     {declared_id!r}\n"
            f"    constant: {constants['SCHEMA_ID']!r}"
        )

    if against is not None:
        if not against.is_file():
            failures.append(f"--against path does not exist: {against}")
        else:
            upstream_sha256 = sha256_of(against)
            if upstream_sha256 != actual_sha256:
                failures.append(
                    "vendored schema differs from the copy it was vendored from\n"
                    f"    vendored: {actual_sha256}\n"
                    f"    upstream: {upstream_sha256}  ({against})"
                )

    return failures


def main(argv: list[str] | None = None) -> int:
    """Entry point. Returns a process exit status."""
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "--against",
        type=Path,
        default=None,
        metavar="PATH",
        help="also compare byte-for-byte with another copy of the schema",
    )
    args = parser.parse_args(argv)

    failures = check(args.against)

    if failures:
        print("FAIL: vendored schema check", file=sys.stderr)
        for failure in failures:
            print(f"  - {failure}", file=sys.stderr)
        return 1

    print(f"ok: {VENDORED_SCHEMA.relative_to(REPO_ROOT)} matches the pinned constants")
    if args.against is not None:
        print(f"ok: byte-identical to {args.against}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
