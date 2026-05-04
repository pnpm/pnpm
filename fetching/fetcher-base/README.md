# @pnpm/fetching.fetcher-base

> Types for pnpm-compatible fetchers

<!--@shields('npm')-->
[![npm version](https://img.shields.io/npm/v/@pnpm/fetching.fetcher-base.svg)](https://npmx.dev/package/@pnpm/fetching.fetcher-base)
<!--/@-->

## Installation

```sh
pnpm add @pnpm/fetching.fetcher-base
```

## Usage

Here's a template for a fetcher using types from `@pnpm/fetching.fetcher-base`:

```ts
import { Resolution } from '@pnpm/resolver-base'
import {
  FetchOptions,
  FetchResult,
} from '@pnpm/fetching.fetcher-base'

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
