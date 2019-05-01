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
    const manifestPath = path.join(importerDir, 'package.json')
    const { data, text } = await readJsonFile(manifestPath)
    const { indent } = detectIndent(text)
    return {
      manifest: data,
      writeImporterManifest: createManifestWriter({
        indent,
        initialManifest: data,
        manifestPath,
      }),
    }
  } catch (err) {
    if (err.code !== 'ENOENT') throw err
  }
  try {
    const manifestPath = path.join(importerDir, 'package.json5')
    const { data, text } = await readJson5File(manifestPath)
    const { indent } = detectIndent(text)
    return {
      manifest: data,
      writeImporterManifest: createManifestWriter({
        indent,
        initialManifest: data,
        manifestPath,
      }),
    }
  } catch (err) {
    if (err.code !== 'ENOENT') throw err
  }
  try {
    const manifestPath = path.join(importerDir, 'package.yaml')
    const manifest = await readPackageYaml(manifestPath)
    return {
      manifest,
      writeImporterManifest: createManifestWriter({ initialManifest: manifest, manifestPath }),
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
      const { indent } = detectIndent(text)
      return {
        manifest: data,
        writeImporterManifest: createManifestWriter({
          indent,
          initialManifest: data,
          manifestPath,
        }),
      }
    }
    case 'package.json5': {
      const { data, text } = await readJson5File(manifestPath)
      const { indent } = detectIndent(text)
      return {
        manifest: data,
        writeImporterManifest: createManifestWriter({
          indent,
          initialManifest: data,
          manifestPath,
        }),
      }
    }
    case 'package.yaml': {
      const manifest = await readPackageYaml(manifestPath)
      return {
        manifest,
        writeImporterManifest: createManifestWriter({ initialManifest: manifest, manifestPath }),
      }
    }
  }
  throw new Error(`Not supported manifest name "${base}"`)
}

function readPackageYaml (filePath: string) {
  return readYamlFile<ImporterManifest>(filePath)
}

function createManifestWriter (
  opts: {
    initialManifest: ImporterManifest,
    indent?: string | number | null | undefined,
    manifestPath: string,
  },
): ((manifest: ImporterManifest) => Promise<void>) {
  const stringifiedInitialManifest = JSON.stringify(opts.initialManifest)
  return async (updatedManifest: ImporterManifest) => {
    if (stringifiedInitialManifest !== JSON.stringify(updatedManifest)) {
      return writeImporterManifest(opts.manifestPath, updatedManifest, { indent: opts.indent })
    }
  }
}
