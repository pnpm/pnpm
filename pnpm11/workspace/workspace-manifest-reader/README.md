# @pnpm/workspace.workspace-manifest-reader

> Reads a workspace manifest file

## Installation

```sh
pnpm add @pnpm/workspace.workspace-manifest-reader
```

## Usage

```ts
import { readWorkspaceManifest, readWorkspaceManifestSync } from '@pnpm/workspace.workspace-manifest-reader'

// Asynchronous read
const workspaceManifest = await readWorkspaceManifest(process.cwd())

// Synchronous read
const workspaceManifestSync = readWorkspaceManifestSync(process.cwd())
```

## License

MIT
