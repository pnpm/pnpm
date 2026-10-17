---
"@pnpm/workspace.state": patch
"@pnpm/deps.status": patch
"@pnpm/installing.commands": patch
"pnpm": patch
---

The `verifyDepsBeforeRun` check now notices an edited manifest, lockfile, or patch file even when the edit kept the file's modification time, as `cp -p`, `rsync -a`, or an archive extraction leave it.

After `pnpm install --filter`, `pnpm run` and `pnpm exec` now notice a changed manifest in a project that install left out. A filtered command for a project that install did not reinstall after a lockfile change now installs first.
