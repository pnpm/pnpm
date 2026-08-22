---
"pacquet": minor
---

`@pnpm/napi` renders pnpm's terminal output and answers `pnpm why` itself, so an embedder no longer needs pnpm's JavaScript for either.

`install` / `rebuild` accept `options.reporter` and render with the same reporter the CLI uses — progress line, packages-diff summary, lifecycle output, the `Done in …` footer. Rendered chunks go to stdout unless the caller passes an `onOutput` callback, which a host that has redirected its own output at the JavaScript level needs so the engine does not write past the redirection. The reporter gained `hideLifecycleOutput`, `ignoredBuildsInstructionText` (pnpm's `approveBuildsInstructionText`), and `hideLinkedPkgsDiff`, a declarative stand-in for the `filterPkgsDiff` callback the addon boundary cannot carry.

`getDependents` returns the reverse dependency trees behind `pnpm why` and `renderDependents` renders them, mirroring the split between `@pnpm/deps.inspection.tree-builder` and `@pnpm/deps.inspection.list`. That split also replaces their `nameFormatter` callback: a consumer asks for the manifest fields it renames by (`manifestFields`), writes `displayName` onto the returned trees, and hands them back to be rendered.

`readLockfile` / `writeLockfile` expose the engine's own lockfile reader and emitter, `filterLockfileByImporters` narrows a lockfile to what the named importers reach, and `readModulesManifest` reads `.modules.yaml` — so an embedder needs neither `@pnpm/lockfile.fs`, `@pnpm/lockfile.filtering`, nor `@pnpm/installing.modules-yaml`.

Top-level lockfile keys pnpm does not define are no longer dropped on a load/save round trip. Tools that drive pnpm programmatically record their own state beside the lockfile — Bit writes a `bit:` block listing the dependencies whose build scripts it has approved — and rewriting the file used to delete it.
