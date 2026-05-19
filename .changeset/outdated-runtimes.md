---
"@pnpm/deps.inspection.outdated": minor
"@pnpm/pkg-manifest.utils": minor
"pnpm": minor
---

`pnpm outdated` and `pnpm update --interactive` now report Node.js, Deno, and Bun runtimes installed as project dependencies (`runtime:` specifiers). Previously these were silently skipped because the npm specifier parser does not understand the `runtime:` protocol, so runtime versions never appeared in the outdated table or the interactive update picker.
