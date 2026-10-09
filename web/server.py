#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Development server for the zkprof-web pages.

    python web/server.py                  # isolated: COOP + COEP
    python web/server.py --no-isolation   # the other deployment context

Serves `web/`, which holds every page, and prints the URLs to open. Point
`--dir` at a single page's directory if you only want one of them:

    python web/server.py --dir web/marshalling-bench

# Why the headers are the default

Browsers coarsen `performance.now()` to blunt timing side channels, and the
coarsening depends on whether the page is cross-origin isolated: roughly 5 us
when it is, roughly 100 us when it is not. That is a 20x difference in the
resolution of every number this project produces, so it is not a deployment
detail -- it is part of the measurement.

Cross-origin isolation requires both headers together:

    Cross-Origin-Opener-Policy: same-origin
    Cross-Origin-Embedder-Policy: require-corp

Both deployment contexts must be runnable, because the constraint the harness
exists to measure includes the un-isolated case. `--no-isolation` serves the
same directory with neither header.

A consequence worth knowing: `require-corp` makes the page refuse subresources
that are not same-origin and do not opt in with their own CORP header. The
harness loads nothing external, so this costs nothing here. Adding a CDN script
later will silently stop loading under the default mode.

# Why it fails rather than waits when the port is taken

This machine hosts other projects with servers of their own. Silently moving to
another port would mean the harness page and the terminal disagree about where
it is; killing the occupant would be touching a process this project did not
start. So it reports and exits.

Stdlib only, no dependencies.
"""

from __future__ import annotations

import argparse
import functools
import http.server
import mimetypes
import sys
from pathlib import Path

DEFAULT_PORT = 8737
DEFAULT_BIND = "127.0.0.1"

# `web/`, not `web/harness/`: there are two pages now, and they share
# `shared.css` one level up. Serving the root is what lets both reach it.
WEB_ROOT = Path(__file__).resolve().parent

ISOLATION_HEADERS: tuple[tuple[str, str], ...] = (
    ("Cross-Origin-Opener-Policy", "same-origin"),
    ("Cross-Origin-Embedder-Policy", "require-corp"),
)

# `WebAssembly.instantiateStreaming` refuses a response whose Content-Type is
# not `application/wasm`. Current CPython knows the extension, but mimetypes
# can be overridden by /etc/mime.types on Linux, and the symptom of getting
# this wrong is a page that will not start with an error that does not
# mention MIME types. Registering it here removes that environment
# dependency.
mimetypes.add_type("application/wasm", ".wasm")


class HarnessHandler(http.server.SimpleHTTPRequestHandler):
    """Static file serving, with the isolation headers when they are on."""

    server_version = "zkprof-harness/0.1"

    def end_headers(self) -> None:
        # `require-corp` is a cache-partitioning signal as well as a security
        # one; without Vary the browser may reuse a response fetched under the
        # other mode.
        if not self.server.no_isolation:  # type: ignore[attr-defined]
            for name, value in ISOLATION_HEADERS:
                self.send_header(name, value)
        self.send_header("Vary", "Cross-Origin-Embedder-Policy")
        # A stale wasm module after a rebuild is a confusing afternoon.
        self.send_header("Cache-Control", "no-store")
        super().end_headers()

    def log_message(self, format: str, *args: object) -> None:
        # The default goes to stderr with the client address; keep it, since
        # "the page fetched nothing" is a real failure mode worth seeing.
        sys.stderr.write(f"{self.address_string()} - {format % args}\n")


class HarnessServer(http.server.ThreadingHTTPServer):
    """Threaded, and deliberately does **not** set `allow_reuse_address`.

    On Windows `SO_REUSEADDR` lets a socket bind to a port another process is
    already listening on. That is the opposite of what this server wants: a
    taken port must be an error, not a silent hijack of someone else's server.
    """

    allow_reuse_address = False
    daemon_threads = True

    def __init__(self, *args: object, no_isolation: bool = False, **kwargs: object) -> None:
        self.no_isolation = no_isolation
        super().__init__(*args, **kwargs)  # type: ignore[arg-type]


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Serve the zkprof-web harness page.",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog=__doc__.split("# Why the headers are the default", 1)[0],
    )
    parser.add_argument(
        "--port",
        type=int,
        default=DEFAULT_PORT,
        help=f"port to listen on (default {DEFAULT_PORT})",
    )
    parser.add_argument(
        "--bind",
        default=DEFAULT_BIND,
        help=f"address to bind (default {DEFAULT_BIND})",
    )
    parser.add_argument(
        "--dir",
        type=Path,
        default=WEB_ROOT,
        help="directory to serve (default: web/, which lists every page)",
    )
    parser.add_argument(
        "--no-isolation",
        action="store_true",
        help="serve without COOP/COEP, so the page is not cross-origin isolated",
    )
    return parser


def main() -> int:
    args = build_parser().parse_args()

    directory = args.dir.resolve()
    if not directory.is_dir():
        print(f"error: {directory} is not a directory.", file=sys.stderr)
        print(
            "Build the wasm modules first: python tools/build_wasm.py",
            file=sys.stderr,
        )
        return 1

    handler = functools.partial(HarnessHandler, directory=str(directory))

    try:
        server = HarnessServer((args.bind, args.port), handler, no_isolation=args.no_isolation)
    except OSError as error:
        if isinstance(error, OSError) and error.errno in _ADDR_IN_USE:
            print(
                f"error: port {args.port} on {args.bind} is already in use.\n"
                f"       {error.strerror}.\n"
                f"\n"
                f"Something else is listening there. Pick another port with "
                f"--port;\nthis server will not kill the occupant.",
                file=sys.stderr,
            )
            return 1
        raise

    mode = "NOT isolated (coarsened clock)" if args.no_isolation else "isolated (fine clock)"
    print(f"serving {directory}")
    print(f"  http://{args.bind}:{args.port}/")

    pages = sorted(path.name for path in directory.iterdir() if (path / "index.html").is_file())
    if pages:
        print("  pages:")
        for name in pages:
            print(f"    http://{args.bind}:{args.port}/{name}/")

    print(f"  deployment context: {mode}")
    if args.no_isolation:
        print("  no COOP/COEP headers; expect a coarser timer_resolution_us")
    else:
        print("  COOP: same-origin, COEP: require-corp")
        print("  cross-origin subresources without CORP will not load (by design)")
    print("Ctrl-C to stop.")

    try:
        server.serve_forever()
    except KeyboardInterrupt:
        print("\nstopped.")
    finally:
        server.server_close()
    return 0


_ADDR_IN_USE = {10048, 98, 48}  # WSAEADDRINUSE, EADDRINUSE (Linux/macOS), macOS


if __name__ == "__main__":
    # `.wasm` is not in every platform's mimetypes database, and a wrong
    # content type makes `WebAssembly.instantiateStreaming` reject the module
    # with an error that says nothing about MIME types.
    mimetypes.add_type("application/wasm", ".wasm")
    sys.exit(main())
