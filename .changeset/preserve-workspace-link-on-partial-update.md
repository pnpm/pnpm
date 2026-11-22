---
"@pnpm/installing.deps-resolver": patch
"pnpm": patch
---

Preserve a workspace dependency's `link:` entry when a run does not target it — e.g. `pnpm update <other-pkg>` (with or without `--recursive`), or a plain install after a root/catalog dependency change — with `injectWorkspacePackages`, instead of spuriously rewriting it to a peer-suffixed `file:` protocol. See pnpm/pnpm#10433.
