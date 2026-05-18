---
"@pnpm/installing.env-installer": minor
"pnpm": minor
---

`configDependencies` now resolve and install one level of `optionalDependencies` declared by the config dependency, with `os`/`cpu`/`libc` platform filtering applied at install time. This unlocks the esbuild/swc-style pattern where a package ships platform-specific binaries via `optionalDependencies` — a config dependency can now do the same and have the matching binary symlinked next to it in the global virtual store, so `require('pkg-platform-arch')` from inside the config dependency resolves correctly.

The env lockfile records all platform variants regardless of host platform, so it remains portable across machines.
