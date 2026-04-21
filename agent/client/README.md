# @pnpm/agent.client

Client library for the pnpm agent server. Reads the local store state, sends it to the server, and writes the received files into the content-addressable store.

## How it works

1. Reads integrity hashes from the local store index (`index.db`).
2. Sends `POST /v1/install` to the pnpm agent server with the project's dependencies and the store integrities.
3. Parses the NDJSON streaming response — `D`-lines (missing file digests) are dispatched to worker downloads against `/v1/files`, `I`-lines are buffered as raw store-index entries, and the final `L`-line yields the resolved lockfile and stats.
4. File download workers write each received file directly to the local CAFS (`files/{hash[:2]}/{hash[2:]}`).
5. Writes store index entries for all new packages in a single SQLite transaction.
6. Returns the resolved lockfile for use with pnpm's headless install (linking phase).

## Usage

This package is used internally by pnpm when the `agent` config option is set. It is not intended to be called directly, but can be used programmatically:

```typescript
import { fetchFromPnpmRegistry } from '@pnpm/agent.client'
import { StoreIndex } from '@pnpm/store.index'

const storeIndex = new StoreIndex('/path/to/store')

const { lockfile, stats } = await fetchFromPnpmRegistry({
  registryUrl: 'http://localhost:4000',
  storeDir: '/path/to/store',
  storeIndex,
  dependencies: { react: '^19.0.0' },
  devDependencies: { typescript: '^5.0.0' },
})

console.log(`Resolved ${stats.totalPackages} packages`)
console.log(`${stats.alreadyInStore} cached, ${stats.filesToDownload} files downloaded`)
// lockfile is ready for headless install
```

## Configuration

Add to `pnpm-workspace.yaml` to enable automatically during `pnpm install`:

```yaml
agent: http://localhost:4000
```
