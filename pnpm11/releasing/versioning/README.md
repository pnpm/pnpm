# @pnpm/releasing.versioning

> Native workspace release management: change intents, release-plan assembly, and changelog writing

[![npm version](https://img.shields.io/npm/v/@pnpm/releasing.versioning.svg)](https://npmx.dev/package/@pnpm/releasing.versioning)

Implements the engine behind `pnpm change` and the bare `pnpm version -r`:
reading and writing changesets-compatible change-intent files from
`.changeset/*.md`, assembling a release plan (direct bumps, dependent
propagation through materialized `workspace:` ranges, fixed groups,
per-package release lanes), and applying it (manifest version updates,
changelog composition, the consumed-intents ledger, and intent-file cleanup).

See the [native monorepo versioning RFC](https://github.com/pnpm/rfcs/pull/18).

## Installation

```sh
pnpm add @pnpm/releasing.versioning
```

## License

MIT
