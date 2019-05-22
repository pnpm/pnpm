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

type WriteImporterManifest = (manifest: ImporterManifest, force?: boolean) => Promise<void>

export default async function readImporterManifest (importerDir: string): Promise<{
  fileName: string,
  manifest: ImporterManifest
  writeImporterManifest: WriteImporterManifest
}> {
  const result = await tryReadImporterManifest(importerDir)
  if (result.manifest !== null) {
    return result as {
      fileName: string,
      manifest: ImporterManifest
      writeImporterManifest: WriteImporterManifest
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
  fileName: string,
  manifest: ImporterManifest | null
  writeImporterManifest: WriteImporterManifest
}> {
  try {
    const manifestPath = path.join(importerDir, 'package.json')
    const { data, text } = await readJsonFile(manifestPath)
    const { indent } = detectIndent(text)
    return {
      fileName: 'package.json',
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
      fileName: 'package.json5',
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
      fileName: 'package.yaml',
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
    fileName: 'package.json',
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

async function readPackageYaml (filePath: string) {
  try {
    return await readYamlFile<ImporterManifest>(filePath)
  } catch (err) {
    if (err.name !== 'YAMLException') throw err
    err.message += `\nin ${filePath}`
    err['code'] = 'ERR_PNPM_YAML_PARSE'
    throw err
  }
}

function createManifestWriter (
  opts: {
    initialManifest: ImporterManifest,
    indent?: string | number | undefined,
    manifestPath: string,
  },
): (WriteImporterManifest) {
  const stringifiedInitialManifest = JSON.stringify(opts.initialManifest)
  return async (updatedManifest: ImporterManifest, force?: boolean) => {
    if (force === true || stringifiedInitialManifest !== JSON.stringify(updatedManifest)) {
      return writeImporterManifest(opts.manifestPath, updatedManifest, { indent: opts.indent })
    }
  }
}
