# @pnpm/log.group

> Prints CI log groups

A lightweight JavaScript utility that provides the ability to group console outputs in various CI platforms.

This library helps in making CI logs more organized and readable by grouping log outputs under specified section names. It supports multiple CI platforms including GitHub Actions, GitLab, Travis, Azure Pipelines, and Buildkite.

API:

```ts
function groupStart(name: string): undefined | () => void
```

## Usage

```ts
import { groupStart } from '@pnpm/log.group'

const endGroup = groupStart('My Section')

console.log('This will be inside the My Section group.')

if (endGroup) endGroup(); // This will end the group.
```

## License

MIT
