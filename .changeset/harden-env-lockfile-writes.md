---
"@pnpm/lockfile.fs": patch
"pnpm": patch
---

Writes that replace the env document of `pnpm-lock.yaml` — the leading document recording config dependencies — now publish through the same hardened writer as the rest of the lockfile, so a symlink appearing at the lockfile's path while the write is in flight can no longer redirect it onto the link's target.
