---
"@pnpm/node.resolver": patch
"@pnpm/plugin-commands-env": patch
---

`parseNodeSpecifier` is moved from `@pnpm/plugin-commands-env` to `@pnpm/node.resolver` and enhanced to support all Node.js version specifier formats. Previously `parseEnvSpecifier` (in `@pnpm/node.resolver`) handled the resolver's parsing, while `parseNodeSpecifier` (in `@pnpm/plugin-commands-env`) was a stricter but now-unused validator. They are now unified into a single `parseNodeSpecifier` in `@pnpm/node.resolver` that supports: exact versions (`22.0.0`), prerelease versions (`22.0.0-rc.4`), semver ranges (`18`, `^18`), LTS codenames (`argon`, `iron`), well-known aliases (`lts`, `latest`), standalone release channels (`nightly`, `rc`, `test`, `v8-canary`, `release`), and channel/version combos (`rc/18`, `nightly/latest`).
