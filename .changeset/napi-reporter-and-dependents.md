---
"pacquet": minor
---

`@pnpm/napi` gained reporter output, reverse dependency queries, and lockfile access.

`install` and `rebuild` accept `options.reporter` and render pnpm's terminal output — progress line, packages-diff summary, lifecycle output, and the `Done in …` footer. Rendered output goes to stdout, or to an `onOutput` callback for a host that writes its own output through JavaScript. New reporting options: `hideLifecycleOutput`, `ignoredBuildsInstructionText`, and `hideLinkedPkgsDiff`.

`getDependents` returns the reverse dependency trees behind `pnpm why`, annotated with the `package.json` fields named in `manifestFields`. `renderDependents` returns those trees rendered as tree, parseable, or JSON output.

`readLockfile` and `writeLockfile` read and write `pnpm-lock.yaml` (or the current lockfile under the virtual store). `filterLockfileByImporters` returns a lockfile narrowed to what the named importers reach. `readModulesManifest` returns the `.modules.yaml` state of an installed `node_modules`.

Top-level lockfile keys pnpm does not define are no longer dropped when a lockfile is loaded and saved, so state a tool records beside pnpm's own keys survives a rewrite.
