# Native macOS file clipboard service 5.6.0

Python-free ARM64 Rust backend. No image decoder, ZIP decompressor, analytics runtime, Python runtime, external HTTP server, or runtime asset directory. The existing compiled Swift `clipboard-helper` remains the sole pasteboard writer.

## Build and test

From this directory, using the isolated toolchain already installed in the workspace:

```sh
export CARGO_HOME=/Users/nchyn/Documents/DSH/windows-build-tools/cargo
export RUSTUP_HOME=/Users/nchyn/Documents/DSH/windows-build-tools/rustup
export MACOSX_DEPLOYMENT_TARGET=12.0
"$CARGO_HOME/bin/cargo" test --offline
"$CARGO_HOME/bin/cargo" build --offline --release
```

Output: `target/release/jpeg-clipboard-service` (native `aarch64-apple-darwin`, macOS 12 minimum deployment target). `Cargo.lock` locks cached dependencies. Cross-architecture Intel binaries are not built here.

## Exact CLI / resources

```text
jpeg-clipboard-service [--port PORT] [--cache-dir PATH] [--helper PATH]
                       [--origin URL] [--json-events] [--no-open]
```

* Default bind `127.0.0.1:48731`, falling back in order through 48735 only on address-in-use. Explicit nondefault port has no fallback; port 0 requests an ephemeral port for tests.
* Cache default `$HOME/Library/Caches/LocalJPEGClipboard`, retained across restarts; file IDs are process-lifetime only.
* Helper default executable-sibling `clipboard-helper`; `--helper` accepts the packaged absolute helper path. Invoked directly, never via shell: `clipboard-helper ABSOLUTE_GENERATED_FILE_PATH`. Success requires zero exit status and JSON stdout with **both** `written:true` and `verified:true`. Report is relayed to HTTP caller. Helper has a 15-second timeout, bounded output and shutdown cancellation. Helper stderr is not logged.
* `--origin URL` validates HTTPS or loopback HTTP without credentials, then retains only canonical scheme/host/port. Native local origin is always accepted. No token/pairing is needed.
* `--json-events` enables stdin parent ownership. GUI must pass it and keep stdin open; EOF or `{"command":"shutdown"}` / `{"cmd":"shutdown"}` stops the process without waiting for stalled workers. Input lines are bounded to 8192 bytes. Legacy unused commands (including clear) emit an error event; native Swift UI owns cache clear while backend is stopped, preserving current pasteboard references. No untrusted command can supply a deletion path.
* `--no-open` retained for old CLI compatibility. Browser opening is disabled by default; GUI owns its Open button.
* `--help` prints usage on stderr. Runtime stdout is JSON Lines only (no banner, access logs, URLs with credentials, or request bodies).

No resources are read from disk at runtime except the helper and cache. Embedded routes:

| GET route | Build input |
| --- | --- |
| `/`, `/index.html`, `/indexv2.html` | `../indexv2.html` |
| `/manifest.json` | `../manifest.json` |
| `/res/jszip.min.js` | `../res/jszip.min.js` |
| `/res/FileSaver.min.js` | `../res/FileSaver.min.js` |
| `/res/jpeg-encode-worker.mjs` | `../res/jpeg-encode-worker.mjs` |

Build script removes entire analytics script blocks (Google/Baidu/gtag), forces `const KV_SIMULATE = true`, and embeds the resulting page. HTML responses add fresh cryptographic script nonces and restrictive CSP, no cross-site static-resource access. Embedded local mode's connect-src is self only.

## Stdout events

```json
{"event":"ready","endpoint":"http://127.0.0.1:48731"}
{"event":"connected","origin":"https://compress.example"}
{"event":"clipboard","name":"folder.zip","bytes":12345}
{"event":"error","message":"..."}
```

`connected` is rate-limited to once per five seconds. Startup failure emits error and exits 1. HTTP validation errors return JSON error responses, not native UI startup-failure events. EOF exits 0 promptly and releases the socket. Normal SIGTERM uses the OS default process termination.

## HTTP protocol and limits

Host must exactly match `127.0.0.1:BOUND_PORT` for every request. POST and OPTIONS require an exact allowed Origin; missing/null/unrelated origins fail. GET rejects nonlocal Origin and `Sec-Fetch-Site: cross-site`. CORS supports POST, OPTIONS, Authorization and Content-Type, plus explicit private-network preflight.

POST endpoints:

* `/api/health`: JSON body, returns service `local-jpeg-clipboard`, version 4, archives true, clipboard `file-url`, codec false.
* `/api/files`: image/jpeg (optional parameters), **4 <= bytes < 20,000,000**. JPEG SOI/EOI signature checked without decoding. Returns 201 with fileId, name, bytes, sha256.
* `/api/archives`: application/zip exactly, **0 < bytes <= 250,000,000**. Raw central metadata validated, <=10000 entries and <=250,000,000 declared expanded bytes. Rejects encrypted, duplicate, NUL, absolute/traversal/dot/backslash/drive paths and symlink entries. ZIP64/multidisk/unsupported malformed central records fail closed. No payload decompression, CRC checking or repacking: original upload is retained byte-for-byte, including compressed payload and comments. Shared single root produces `root.zip`; mixed roots produce `文件夹.zip`. SHA256 is also returned for ZIP.
* `/api/clipboard/file`: JSON `{ "fileId": "process-issued-id" }`. Unknown IDs return 404; no caller-provided filesystem path is accepted.

Content-Length is required; Transfer-Encoding and duplicate HTTP headers are rejected. Header cap 16KiB; JSON cap 4096 bytes. HTTP/1.0 and 1.1 supported, one request per connection, connection closed after response. Fixed maximum eight per-request workers, 15-second socket read/write idle timeouts, 15-second header deadline, 60-second total request-body deadline. Slow uploads cannot block health or the accept loop. Uploads/clipboard writes serialize only cache operations.

Cache cap **1,000,000,000 bytes**, counting all regular descendants and in-flight uploads under one lock. Random 128-bit OS-generated hex IDs; incremental SHA256; partial files use exclusive creation and are cleaned after errors. Cache directories are private 0700, files 0600. Descriptor-relative openat/fstatat/renameat/unlinkat with O_NOFOLLOW validate every cache path component, prevent symlink directory traversal and root replacement, and reject hard-linked or special cache entries. Refuse suspicious caches rather than deleting around them. No automatic eviction of files potentially referenced by pasteboard.

## Tests

`tests/service.rs` starts the actual Rust executable on an ephemeral loopback port and a fake shell helper that only returns protocol JSON. It never invokes the real clipboard helper or writes the system pasteboard. Coverage includes embedded assets/analytics/local KV/nonces, strict origins and HTTP framing, original JPEG bytes/hash, fake helper protocol, original ZIP bytes/root name, unsafe ZIP paths/duplicates/encryption/declared expansion, 1GB cache cap/symlink fail-closed, parallel health during stalled headers/uploads, prompt stdin EOF shutdown, and all-five-ports-busy error. All six integration tests pass on native ARM64 macOS.

Security implementation note: cached zip 2.6.1 deduplicates names into an IndexMap before `ZipArchive::len()`/iteration. `validate_central` therefore validates original raw central records **before** the crate API, so duplicate entries or hidden expanded sizes cannot disappear from checks.
