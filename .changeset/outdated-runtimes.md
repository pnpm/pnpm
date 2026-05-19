---
"@pnpm/deps.inspection.outdated": minor
"@pnpm/installing.client": minor
"@pnpm/resolving.default-resolver": minor
"@pnpm/resolving.resolver-base": minor
"@pnpm/resolving.npm-resolver": minor
"@pnpm/resolving.git-resolver": minor
"@pnpm/resolving.tarball-resolver": minor
"@pnpm/resolving.local-resolver": minor
"@pnpm/engine.runtime.node-resolver": minor
"@pnpm/engine.runtime.bun-resolver": minor
"@pnpm/engine.runtime.deno-resolver": minor
"@pnpm/pkg-manifest.utils": minor
"pnpm": minor
---

`pnpm outdated` and `pnpm update --interactive` now report Node.js, Deno, and Bun runtimes installed as project dependencies (`runtime:` specifiers). Previously these were silently skipped because the npm specifier parser did not understand the `runtime:` protocol, so runtime versions never appeared in the outdated table or the interactive update picker.

Internally, the outdated check is now resolver-driven: `@pnpm/resolving.resolver-base` defines a `ResolveLatestFunction` shape (with `LatestQuery` input — `{ wantedDependency, compatible? }` — and `LatestInfo` result — `{ latestManifest? }`), and every protocol resolver (npm, jsr, named-registry, git, tarball, local, node/bun/deno runtimes) exports its own `resolveLatest*` function alongside its `resolve*`. `@pnpm/resolving.default-resolver` composes them into a single dispatcher, exposed through `@pnpm/installing.client` as `createResolver(...).resolveLatest`.

Each resolver decides whether it owns the dep and what "latest" means for its protocol; the outdated command derives `current` / `wanted` display values from the lockfile snapshot (`pkgSnapshot.version` for semver protocols, raw ref for URL-shaped ones) and uses raw ref equality for the "lockfile changed" check, so protocol knowledge stays inside each resolver instead of the command.
