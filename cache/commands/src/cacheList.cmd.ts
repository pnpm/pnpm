import path from 'path'
import fastGlob from 'fast-glob'

export async function cacheListCmd (opts: { cacheDir: string }, filter: string[]): Promise<string> {
  const patterns = filter.length ? filter.map((filter) => `**/${filter}.json`) : ['**']
  console.log(patterns)
  const metaFiles = await fastGlob(patterns, {
    cwd: path.join(opts.cacheDir, 'metadata-v1.1'),
  })
  return metaFiles.join('\n')
}
