import { ImporterManifest } from '@pnpm/types'
import loadJsonFile from 'load-json-file'
import path = require('path')

export async function readImporterManifest (filename: string) {
  return loadJsonFile<ImporterManifest>(filename)
}

export async function readImporterManifestFromDir (dir: string) {
  return readImporterManifest(path.join(dir, 'package.json'))
}

export async function safeReadImporterManifest (filename: string): Promise<ImporterManifest | null> {
  try {
    return await readImporterManifest(filename)
  } catch (err) {
    if (err['code'] !== 'ENOENT') throw err
    return null
  }
}

export function safeReadImporterManifestFromDir (dir: string) {
  return safeReadImporterManifest(path.join(dir, 'package.json'))
}
