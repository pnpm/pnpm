# @pnpm/assert-project

> Utils for testing projects that use pnpm

## Installation

```
pnpm install -D @pnpm/assert-project
```

## Usage

```ts
import test = require('tape')
import { assertProject } from '@pnpm/assert-project'

test('...', async t => {
  // ...
  const project = assertProject(t, pathToProject)

  await project.has('foo')
  // Test fails if project has no foo in node_modules
})
```

## License

MIT
