# @pnpm/resolving.resolver-base

> Types for pnpm-compatible resolvers

<!--@shields('npm')-->
[![npm version](https://img.shields.io/npm/v/@pnpm/resolving.resolver-base.svg)](https://npmx.dev/package/@pnpm/resolving.resolver-base)
<!--/@-->

## Installation

```sh
pnpm add @pnpm/resolving.resolver-base
```

## Usage

Here's a template of a resolver using types from `@pnpm/resolving.resolver-base`:

```ts
import {
  ResolveOptions,
  ResolveResult,
  WantedDependency,
} from '@pnpm/resolving.resolver-base'

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
    normalizedBareSpecifier,
  }
}
```

## License

MIT
