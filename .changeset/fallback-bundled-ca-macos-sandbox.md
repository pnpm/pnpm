---
"pacquet": patch
---

Fixed installs failing on macOS when system trust evaluation is restricted, such as in a sandbox. Network requests now fall back to bundled CA roots when the platform trust service is unreachable, and honor custom `ca` certificates directly [#15329](https://github.com/pnpm/pnpm/issues/15329).
