---
"pacquet": patch
---

`pnpm install` and the `verifyDepsBeforeRun` check now notice an edited manifest, lockfile, or patch file even when the edit kept the file's modification time, as `cp -p`, `rsync -a`, or an archive extraction leave it.

After `pnpm install --filter`, `pnpm run` and `pnpm exec` now notice a changed manifest in a project that install left out.
