---
"@pnpm/auth.commands": minor
"pnpm": minor
---

Implement the documented `pnpm login --scope <scope>` flag. The scope is normalized (a leading `@` is added if missing; blank values are ignored) and an `@<scope>:registry=<registry>` mapping is written to the pnpm auth file alongside the auth token. Subsequent installs of `@<scope>/*` packages then route to the chosen registry. Previously `pnpm login --scope foo` errored with `Unknown option: 'scope'` despite the flag being listed in the online documentation [#11716](https://github.com/pnpm/pnpm/issues/11716).
