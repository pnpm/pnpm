---
"@pnpm/installing.deps-resolver": patch
"pnpm": patch
---

Fixed peer dependency resolution with `autoInstallPeers` when a workspace package depends on a version of a package that a transitive dependency's self-contained closure also provides for itself. The peer providers that are attached to the root project for reuse are no longer peer-resolved a second time in the root context, so packages inside such a closure no longer get their peers bound to the root project's incompatible version [#4993](https://github.com/pnpm/pnpm/issues/4993).
