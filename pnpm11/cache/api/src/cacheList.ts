import fs from 'node:fs'

import { decodeRegistry, encodeRegistry } from '@pnpm/resolving.npm-resolver'
import { glob } from 'tinyglobby'

export async function cacheListRegistries (opts: { cacheDir: string, registry?: string, registries?: boolean }): Promise<string> {
  return fs.readdirSync(opts.cacheDir, { withFileTypes: true })
    .filter((entry) => entry.isDirectory())
    .map((entry) => decodeRegistry(entry.name))
    .sort()
    .join('\n')
}

export async function cacheList (opts: { cacheDir: string, registry?: string, registries?: boolean }, filter: string[]): Promise<string> {
  const metaFiles = await findMetadataFiles(opts, filter)
  return metaFiles.sort().join('\n')
}

export async function findMetadataFiles (opts: { cacheDir: string, registry?: string }, filter: string[]): Promise<string[]> {
  const prefix = opts.registry ? encodeRegistry(opts.registry) : '*'
  const patterns = filter.length ? filter.map((filter) => `${prefix}/${filter}.jsonl`) : [`${prefix}/**`]
  const metaFiles = await glob(patterns, {
    cwd: opts.cacheDir,
    expandDirectories: false,
  })
  return metaFiles
}
