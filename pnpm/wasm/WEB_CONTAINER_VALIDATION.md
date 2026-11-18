# WebContainer follow-up measurements

Measured with the packaged Rust WASM CLI in Playwright Chromium 159.0.8154.6
and WebContainer Node.js 24.22.2 (`webcontainer` runtime `0df13572`). The CLI
reported version 12.22.0.

`pnpm/tasks/ecosystem-e2e/webcontainer/metadata-bench.mjs` ran 30 batches of 256
files in the actual WebContainer. Each value is the median or p95 elapsed time
per file across batches.

| Host operation | Median | p95 |
| --- | ---: | ---: |
| `lstatSync` | 3.83 µs | 8.48 µs |
| `openSync` + `fstatSync` + `closeSync` | 2.91 µs | 5.55 µs |
| `chmodSync` | 1.09 µs | 2.11 µs |

These timings isolate Node filesystem metadata calls; they are not a full
package extraction benchmark or a count of calls made by pnpm during an
install. `pnpm wasm` must retain the permission checks and mode restoration
needed by its filesystem contract. The measurements do not support removing
those operations.

The bridge microbenchmark in `pnpm/wasm/host/guest-operations.bench.mjs`
compares one 64 KiB response through the JSON byte-array path and the binary
stream path over the same worker and shared-memory RPC. The JSON control message
is 234,015 bytes; the binary path's JSON control message is 55 bytes, plus the
64 KiB payload copied separately. Two 100-pair runs on Node.js 26.12.0 measured
JSON medians of 12.91 ms and 22.93 ms, versus binary medians of 0.42 ms and
0.64 ms. Their p95 values were 26.87 ms and 46.58 ms for JSON, versus 1.57 ms
and 4.74 ms for binary. Absolute times varied with host load; the reduction in
control payload and both paired runs support using the binary path. This is a
Node worker benchmark, not a browser download benchmark.

The packaged CLI browser runs passed isolated and hoisted linker installs, the install, workspace, script, lifecycle,
interactive build approval, package-manager pin, frontend build, and private
mock-registry workflows. The added registry flow covers interactive login and
its `adduser` alias, whoami, profile read, token listing, documentation URL
output when the browser cannot open it, authenticated publish, unpublish,
access, owner, team, distribution tags, deprecation, undeprecation and logout.
It checks missing authentication errors for whoami, profile and token listing.
Packaged workflows also verify project initialization, dependency inspection
(`why`, `peers`, `licenses`, `fund` and CycloneDX `sbom`), permission listing
and no-pending approval, ignored-build listing, package and script
manifest edits (including unsafe-key rejection), completion generation, task
status, package-store index/content reads and missing/invalid cases, metadata-cache
list/view/delete/prune operations (including path traversal rejection), the `licences`
and `la` aliases, the `i`, `ls`, `rm`, `un`, `uni` and `uninstall` aliases, and
store integrity detection and pruning. The inspection flow also covers
`install-test` and its `it` alias, plus `ci` and its `clean-install` and
`install-clean` aliases in an isolated project. It does not cover every command
or each operation's full error matrix; see `webcontainer-commands.tsv`.

`clean` and its `purge` alias fail on the installed workspace while removing
`node_modules/.pnpm`, reporting `Directory not empty`. The isolated `ci`
workflow succeeds; workspace cleanup remains an observed filesystem failure.

The WebContainer host does not support the Linux `/proc/self/fd` permission
grant path. The fallback checks descendant components with `lstat` before
opening by path, so it is not descriptor-stable across a concurrent rename.
This audit did not run an adversarial rename interleaving in the actual
WebContainer; the permission inheritance race remains unverified.

Five host network restrictions were probed against the packaged CLI: an
explicit HTTPS proxy, disabled TLS verification, a pinned local address, custom
TLS trust roots, and client TLS identities. Each command returned its capability
error before changing project files or PNPM_HOME contents.

Cargo integration reached the host process launch and failed because `cargo`
is not installed in the WebContainer. The fixture verifies that the command
leaves project files and PNPM_HOME contents unchanged. It does not establish
the behavior of Cargo operations when a Cargo executable is available.
