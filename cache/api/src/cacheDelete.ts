import path from 'path'
import fs from 'fs'
import { findMetadataFiles } from './cacheList'

export async function cacheDelete (opts: { cacheDir: string, registry?: string }, filter: string[]): Promise<string> {
  const metaFiles = await findMetadataFiles(opts, filter)
  for (const metaFile of metaFiles) {
    fs.unlinkSync(path.join(opts.cacheDir, metaFile))
  }
  return metaFiles.sort().join('\n')
}
