# @pnpm/deps.github-actions

> Discover and update GitHub Actions dependencies

[![npm version](https://img.shields.io/npm/v/@pnpm/deps.github-actions.svg)](https://npmx.dev/package/@pnpm/deps.github-actions)

## Installation

```sh
pnpm add @pnpm/deps.github-actions
```

## Usage

```ts
import {
  findOutdatedGitHubActions,
  updateGitHubActions,
} from '@pnpm/deps.github-actions'

const outdated = await findOutdatedGitHubActions({
  dir: process.cwd(),
})

await updateGitHubActions({
  dir: process.cwd(),
})
```

The package scans workflow files in `.github/workflows` and follows referenced local reusable workflows and composite actions. Only `uses` fields in jobs and steps are treated as dependencies.

Updates are always pinned to an exact commit SHA. The corresponding semantic version tag is written in a comment:

```yaml
- uses: actions/checkout@9c091bb21b7c1c1d1991bb908d89e4e9dddfe3e0 # v7.0.0
```

By default, `updateGitHubActions` selects the newest caret-compatible release. This keeps `0.5.x` releases below `0.6.0`, since pre-1.0 minor releases may contain breaking changes. Set `latest: true` to allow incompatible updates. `findOutdatedGitHubActions` reports the newest release by default; set `compatible: true` to report only caret-compatible updates.

## License

MIT
