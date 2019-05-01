import { ImporterManifest } from '@pnpm/types'
import writeImporterManifest from '@pnpm/write-importer-manifest'
import detectIndent = require('detect-indent')
import fs = require('fs')
import { Stats } from 'fs'
import isWindows = require('is-windows')
import path = require('path')
import readYamlFile from 'read-yaml-file'
import { promisify } from 'util'
import {
  readJson5File,
  readJsonFile,
} from './readFile'

const stat = promisify(fs.stat)

export default async function readImporterManifest (importerDir: string): Promise<{
  manifest: ImporterManifest
  writeImporterManifest: (manifest: ImporterManifest) => Promise<void>
}> {
  const result = await tryReadImporterManifest(importerDir)
  if (result.manifest !== null) {
    return result as {
      manifest: ImporterManifest
      writeImporterManifest: (manifest: ImporterManifest) => Promise<void>
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
  manifest: ImporterManifest | null
  writeImporterManifest: (manifest: ImporterManifest) => Promise<void>
}> {
  try {
    const filePath = path.join(importerDir, 'package.json')
    const { data, text } = await readJsonFile(filePath)
    const indent = detectIndent(text).indent
    return {
      manifest: data,
      writeImporterManifest: (manifest: ImporterManifest) => writeImporterManifest(filePath, manifest, { indent }),
    }
  } catch (err) {
    if (err.code !== 'ENOENT') throw err
  }
  try {
    const filePath = path.join(importerDir, 'package.json5')
    const { data, text } = await readJson5File(filePath)
    const indent = detectIndent(text).indent
    return {
      manifest: data,
      writeImporterManifest: (manifest: ImporterManifest) => writeImporterManifest(filePath, manifest, { indent }),
    }
  } catch (err) {
    if (err.code !== 'ENOENT') throw err
  }
  try {
    const filePath = path.join(importerDir, 'package.yaml')
    return {
      manifest: await readPackageYaml(filePath),
      writeImporterManifest: writeImporterManifest.bind(null, filePath),
    }
  } catch (err) {
    if (err.code !== 'ENOENT') throw err
  }
  if (isWindows()) {
    // ENOTDIR isn't used on Windows, but pnpm expects it.
    let s: Stats | undefined
    try {
      s = await stat(importerDir)
    } catch (err) {
      // Ignore
    }
    if (s && !s.isDirectory()) {
      const err = new Error(`"${importerDir}" is not a directory`)
      err['code'] = 'ENOTDIR' // tslint:disable-line
      throw err
    }
  }
  const filePath = path.join(importerDir, 'package.json')
  return {
    manifest: null,
    writeImporterManifest: writeImporterManifest.bind(null, filePath),
  }
}

export async function readExactImporterManifest (manifestPath: string) {
  const base = path.basename(manifestPath).toLowerCase()
  switch (base) {
    case 'package.json': {
      const { data, text } = await readJsonFile(manifestPath)
      const indent = detectIndent(text).indent
      return {
        manifest: data,
        writeImporterManifest: (manifest: ImporterManifest) => writeImporterManifest(manifestPath, manifest, { indent }),
      }
    }
    case 'package.json5': {
      const { data, text } = await readJson5File(manifestPath)
      const indent = detectIndent(text).indent
      return {
        manifest: data,
        writeImporterManifest: (manifest: ImporterManifest) => writeImporterManifest(manifestPath, manifest, { indent }),
      }
    }
    case 'package.yaml': {
      return {
        manifest: await readPackageYaml(manifestPath),
        writeImporterManifest: writeImporterManifest.bind(null, manifestPath),
      }
    }
  }
  throw new Error(`Not supported manifest name "${base}"`)
}

function readPackageYaml (filePath: string) {
  return readYamlFile<ImporterManifest>(filePath)
}
