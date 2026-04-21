---
"pnpm": patch
---

When a project's `packageManager` field selects pnpm v11 or newer, commands that v10 would have passed through to npm (`version`, `login`, `logout`, `publish`, `unpublish`, `deprecate`, `dist-tag`, `docs`, `ping`, `search`, `star`, `stars`, `unstar`, `whoami`, etc.) are now routed through `main()` so `switchCliVersion` can hand them to the wanted pnpm, which implements them natively. Previously they silently shelled out to npm — making, for example, `pnpm version --help` print npm's help on a project with `packageManager: pnpm@11.0.0-rc.3` [#11328](https://github.com/pnpm/pnpm/issues/11328).
