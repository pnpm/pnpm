import { ImporterManifest } from '@pnpm/types'
import readJsonFile = require('load-json-file')
import path = require('path')
import readJson5File = require('read-json5-file')
import readYamlFile from 'read-yaml-file'

export default async function readImporterManifest (importerDir: string): Promise<{
  fileName: string
  manifest: ImporterManifest
}> {
  const result = await tryReadImporterManifest(importerDir)
  if (result.manifest !== null) {
    return result as {
      fileName: string
      manifest: ImporterManifest
    }
  }
  const err = new Error(`No package.json (or package.yaml, or package.json5) was found in "${importerDir}".`)
  err['code'] = 'ERR_PNPM_NO_IMPORTER_MANIFEST_FOUND'
  throw err
}

export async function tryReadImporterManifest (importerDir: string): Promise<{
  fileName: string
  manifest: ImporterManifest | null
}> {
  try {
    return {
      fileName: 'package.json',
      manifest: await readJsonFile<ImporterManifest>(path.join(importerDir, 'package.json')),
    }
  } catch (err) {
    if (err.code !== 'ENOENT') throw err
  }
  try {
    return {
      fileName: 'package.json5',
      manifest: await readJson5File<ImporterManifest>(path.join(importerDir, 'package.json5')),
    }
  } catch (err) {
    if (err.code !== 'ENOENT') throw err
  }
  try {
    return {
      fileName: 'package.yaml',
      manifest: await readYamlFile<ImporterManifest>(path.join(importerDir, 'package.yaml')),
    }
  } catch (err) {
    if (err.code !== 'ENOENT') throw err
  }
  return { fileName: 'package.json', manifest: null }
}
