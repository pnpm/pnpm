# TypeScript Package Configuration

pax replaces `package.json` with `package.config.ts` — a fully typed configuration file that provides autocompletion, type-checking, and the full power of TypeScript at the config level.

## Status

This feature is planned but not yet implemented. This page captures the design direction.

## Design goals

1. **Strong typing.** The configuration schema is defined as TypeScript types, so editors provide autocompletion and type-checking out of the box.
2. **Transparent compilation.** `package.config.ts` compiles down to a standard `package.json` that npm, pnpm, and other tools can consume without modification.
3. **Composability.** Common configuration patterns (shared company defaults, base configs for libraries vs. apps, etc.) can be extracted into reusable TypeScript modules and imported by `package.config.ts` files.
4. **No runtime dependency.** The TS config is evaluated at build/install time. It does not ship to consumers or affect the runtime dependency graph.

## Planned API

```ts
// package.config.ts
import { definePackage } from 'pax/config'

export default definePackage({
  name: 'my-app',
  version: '1.0.0',
  dependencies: {
    react: '^18.0.0',
    next: '^14.0.0',
  },
  foreign: {
    cargo: {
      'wasm-bindgen': '^0.2',
    },
    pip: {
      numpy: '>=1.24',
    },
  },
})
```

## Shared config utilities

It should be possible to publish and import shared configuration fragments:

```ts
// @my-org/pkg-config-base/index.ts
import { definePartial } from 'pax/config'

export const base = definePartial({
  license: 'MIT',
  author: 'My Org',
  repository: { type: 'git', url: 'https://github.com/my-org/monorepo' },
})
```

```ts
// packages/my-lib/package.config.ts
import { definePackage } from 'pax/config'
import { base } from '@my-org/pkg-config-base'

export default definePackage({
  ...base,
  name: '@my-org/my-lib',
  version: '2.0.0',
  dependencies: {
    zod: '^3.0.0',
  },
})
```

## Open questions

- Can shared TS config files interact with the actual source code (e.g., auto-detect exports)? Likely not in phase 1, but worth exploring.
- Should the file be named `package.config.ts`, `pax.config.ts`, or something else?
- How should `package.config.ts` coexist with `package.json` during migration? Probably `package.config.ts` takes precedence when present, and the generated `package.json` is written alongside it (gitignored or not — TBD).
