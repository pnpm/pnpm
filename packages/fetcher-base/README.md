# @pnpm/fetcher-base

> Types for pnpm-compatible fetchers

<!--@shields('npm')-->
[![npm version](https://img.shields.io/npm/v/@pnpm/fetcher-base.svg)](https://www.npmjs.com/package/@pnpm/fetcher-base)
<!--/@-->

## Installation

```sh
pnpm add @pnpm/fetcher-base
```

## Usage

Here's a template for a fetcher using types from `@pnpm/fetcher-base`:

```ts
import { Resolution } from '@pnpm/resolver-base'
import {
  FetchOptions,
  FetchResult,
} from '@pnpm/fetcher-base'

export async function demoFetcher (
  resolution: Resolution,
  targetFolder: string,
  opts: FetchOptions,
): Promise<FetchResult> {
  // ...
  return {
    filesIndex,
    tempLocation,
  }
}
```

## License

MIT
