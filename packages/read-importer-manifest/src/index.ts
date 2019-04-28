import { ImporterManifest } from '@pnpm/types'
import readJsonFile = require('load-json-file')
import path = require('path')
import readJson5File = require('read-json5-file')
import readYamlFile from 'read-yaml-file'

export interface ReadImporterManifestResult {
  fileName: string
  manifest: ImporterManifest | null
}

export default async function readImporterManifest (importerDir: string): Promise<ReadImporterManifestResult> {
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
