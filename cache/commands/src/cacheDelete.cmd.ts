import path from 'path'
import fs from 'fs'
import { findMetadataFiles } from './cacheList.cmd'

export async function cacheDeleteCmd (opts: { cacheDir: string, registry?: string }, filter: string[]): Promise<string> {
  const metaFiles = await findMetadataFiles(opts, filter)
  const baseDir = path.join(opts.cacheDir, 'metadata')
  for (const metaFile of metaFiles) {
    fs.unlinkSync(path.join(baseDir, metaFile))
  }
  return metaFiles.sort().join('\n')
}
