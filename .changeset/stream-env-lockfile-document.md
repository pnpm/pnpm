---
"pacquet": patch
---

Commands in a project that pins a pnpm version no longer read the whole `pnpm-lock.yaml` to get at the leading env document. Only the first chunk of the file is read, so the cost no longer grows with the lockfile: reading the env document out of an 8 MB lockfile takes ~15µs instead of ~390µs.
