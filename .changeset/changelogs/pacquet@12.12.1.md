## 12.12.1

This release adds OIDC authentication to `pnpm dist-tag` and the `tool:` protocol. `pnpm publish` now checks which registry an `NPM_ID_TOKEN` was issued for before using it.

### Patch Changes

- `pnpm publish` now uses an `NPM_ID_TOKEN` only if its `aud` claim matches the selected registry, such as `npm:registry.npmjs.org`. A token for another registry is skipped with a warning, and pnpm falls back to the configured credentials.

- `pnpm dist-tag add`, `rm`, and `ls` now support OIDC authentication in CI. Enable "Allow npm dist-tag" in the package's trusted publishing settings to manage tags without a stored npm token.

- Added the `tool:` protocol for the tools pnpm downloads itself: Node.js, Bun, Deno, and Yarn. `pnpm add bun@tool:1.3.0`, `pnx node@tool:22`, and `pnx yarn@tool:4` work like the `runtime:` spelling, which stays supported as an alias. A lockfile written for `runtime:` stays up to date when `package.json` switches to `tool:`, and the other way around.

  A named registry can no longer be called `tool`.
