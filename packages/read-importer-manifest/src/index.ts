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

export async function readImporterManifestOnly (importerDir: string): Promise<ImporterManifest> {
  const { manifest } = await readImporterManifest(importerDir)
  return manifest
}

export async function tryReadImporterManifest (importerDir: string): Promise<{
  fileName: string
  manifest: ImporterManifest | null
}> {
  try {
    return {
      fileName: 'package.json',
      manifest: await readPackageJson(path.join(importerDir, 'package.json')),
    }
  } catch (err) {
    if (err.code !== 'ENOENT') throw err
  }
  try {
    return {
      fileName: 'package.json5',
      manifest: await readPackageJson5(path.join(importerDir, 'package.json5')),
    }
  } catch (err) {
    if (err.code !== 'ENOENT') throw err
  }
  try {
    return {
      fileName: 'package.yaml',
      manifest: await readPackageYaml(path.join(importerDir, 'package.yaml')),
    }
  } catch (err) {
    if (err.code !== 'ENOENT') throw err
  }
  return { fileName: 'package.json', manifest: null }
}

export async function readExactImporterManifest (manifestPath: string) {
  const base = path.basename(manifestPath).toLowerCase()
  switch (base) {
    case 'package.json':
      return {
        fileName: 'package.json',
        manifest: await readPackageJson(manifestPath),
      }
    case 'package.json5':
      return {
        fileName: 'package.json5',
        manifest: await readPackageJson5(manifestPath),
      }
    case 'package.yaml':
      return {
        fileName: 'package.yaml',
        manifest: await readPackageYaml(manifestPath),
      }
  }
  throw new Error(`Not supported manifest name "${base}"`)
}

function readPackageJson (filePath: string) {
  return readJsonFile<ImporterManifest>(filePath)
}

function readPackageJson5 (filePath: string) {
  return readJson5File<ImporterManifest>(filePath)
}

function readPackageYaml (filePath: string) {
  return readYamlFile<ImporterManifest>(filePath)
}
