import path from 'path'
import getRegistryName from 'encode-registry'
import fastGlob from 'fast-glob'

export async function cacheListCmd (opts: { cacheDir: string, registry?: string }, filter: string[]): Promise<string> {
  const metaFiles = await findMetadataFiles(opts, filter)
  return metaFiles.sort().join('\n')
}

export async function findMetadataFiles (opts: { cacheDir: string, registry?: string }, filter: string[]): Promise<string[]> {
  const prefix = opts.registry ? `${getRegistryName(opts.registry)}` : '*'
  const patterns = filter.length ? filter.map((filter) => `${prefix}/${filter}.json`) : [`${prefix}/**`]
  const metaFiles = await fastGlob(patterns, {
    cwd: path.join(opts.cacheDir, 'metadata'),
  })
  return metaFiles
}
