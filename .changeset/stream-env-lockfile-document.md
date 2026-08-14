---
"pacquet": patch
---

Commands in a project that pins a pnpm version no longer read the whole `pnpm-lock.yaml` to get at the leading env document. Reading stops at the end of that document, so the cost no longer grows with the rest of the lockfile: reading the env document out of an 8 MB lockfile takes ~15µs instead of ~390µs.
