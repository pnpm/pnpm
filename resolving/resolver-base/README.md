# @pnpm/resolver-base

> Types for pnpm-compatible resolvers

<!--@shields('npm')-->
[![npm version](https://img.shields.io/npm/v/@pnpm/resolver-base.svg)](https://www.npmjs.com/package/@pnpm/resolver-base)
<!--/@-->

## Installation

```sh
pnpm add @pnpm/resolver-base
```

## Usage

Here's a template of a resolver using types from `@pnpm/resolver-base`:

```ts
import {
  ResolveOptions,
  ResolveResult,
  WantedDependency,
} from '@pnpm/resolver-base'

export async function testResolver (
  wantedDependency: WantedDependency,
  opts: ResolveOptions,
): Promise<ResolveResult> {
  // ...
  return {
    id,
    resolution,
    package,
    latest,
    normalizedPref,
  }
}
```

## License

MIT
