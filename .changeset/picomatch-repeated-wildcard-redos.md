---
"pacquet": patch
---

Matching a glob that repeats a wildcard, such as `+(*)z` in the workspace `packages` patterns, no longer hangs on long paths. The bundled picomatch is patched for a [security advisory](https://github.com/advisories/GHSA-fc5q-39xc-vjpf).
