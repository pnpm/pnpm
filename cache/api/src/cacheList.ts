import fs from 'fs'
import getRegistryName from 'encode-registry'
import fastGlob from 'fast-glob'

export async function cacheListRegistries (opts: { cacheDir: string, registry?: string, registries?: boolean }): Promise<string> {
  return fs.readdirSync(opts.cacheDir).sort().join('\n')
}

export async function cacheList (opts: { cacheDir: string, registry?: string, registries?: boolean }, filter: string[]): Promise<string> {
  const metaFiles = await findMetadataFiles(opts, filter)
  return metaFiles.sort().join('\n')
}

export async function findMetadataFiles (opts: { cacheDir: string, registry?: string }, filter: string[]): Promise<string[]> {
  const prefix = opts.registry ? `${getRegistryName(opts.registry)}` : '*'
  const patterns = filter.length ? filter.map((filter) => `${prefix}/${filter}.json`) : [`${prefix}/**`]
  const metaFiles = await fastGlob(patterns, {
    cwd: opts.cacheDir,
  })
  return metaFiles
}
