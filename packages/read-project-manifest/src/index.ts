import PnpmError from '@pnpm/error'
import { ProjectManifest } from '@pnpm/types'
import writeProjectManifest from '@pnpm/write-project-manifest'
import detectIndent = require('detect-indent')
import equal = require('fast-deep-equal')
import fs = require('fs')
import { Stats } from 'fs'
import isWindows = require('is-windows')
import path = require('path')
import readYamlFile from 'read-yaml-file'
import sortKeys = require('sort-keys')
import { promisify } from 'util'
import {
  readJson5File,
  readJsonFile,
} from './readFile'

const stat = promisify(fs.stat)

type WriteProjectManifest = (manifest: ProjectManifest, force?: boolean) => Promise<void>

export default async function readProjectManifest (projectDir: string): Promise<{
  fileName: string,
  manifest: ProjectManifest
  writeProjectManifest: WriteProjectManifest
}> {
  const result = await tryReadProjectManifest(projectDir)
  if (result.manifest !== null) {
    return result as {
      fileName: string,
      manifest: ProjectManifest
      writeProjectManifest: WriteProjectManifest
    }
  }
  throw new PnpmError('NO_IMPORTER_MANIFEST_FOUND',
    `No package.json (or package.yaml, or package.json5) was found in "${projectDir}".`)
}

export async function readProjectManifestOnly (projectDir: string): Promise<ProjectManifest> {
  const { manifest } = await readProjectManifest(projectDir)
  return manifest
}

export async function tryReadProjectManifest (projectDir: string): Promise<{
  fileName: string,
  manifest: ProjectManifest | null
  writeProjectManifest: WriteProjectManifest
}> {
  try {
    const manifestPath = path.join(projectDir, 'package.json')
    const { data, text } = await readJsonFile(manifestPath)
    const { indent } = detectIndent(text)
    return {
      fileName: 'package.json',
      manifest: data,
      writeProjectManifest: createManifestWriter({
        indent,
        initialManifest: data,
        manifestPath,
      }),
    }
  } catch (err) {
    if (err.code !== 'ENOENT') throw err
  }
  try {
    const manifestPath = path.join(projectDir, 'package.json5')
    const { data, text } = await readJson5File(manifestPath)
    const { indent } = detectIndent(text)
    return {
      fileName: 'package.json5',
      manifest: data,
      writeProjectManifest: createManifestWriter({
        indent,
        initialManifest: data,
        manifestPath,
      }),
    }
  } catch (err) {
    if (err.code !== 'ENOENT') throw err
  }
  try {
    const manifestPath = path.join(projectDir, 'package.yaml')
    const manifest = await readPackageYaml(manifestPath)
    return {
      fileName: 'package.yaml',
      manifest,
      writeProjectManifest: createManifestWriter({ initialManifest: manifest, manifestPath }),
    }
  } catch (err) {
    if (err.code !== 'ENOENT') throw err
  }
  if (isWindows()) {
    // ENOTDIR isn't used on Windows, but pnpm expects it.
    let s: Stats | undefined
    try {
      s = await stat(projectDir)
    } catch (err) {
      // Ignore
    }
    if (s && !s.isDirectory()) {
      const err = new Error(`"${projectDir}" is not a directory`)
      err['code'] = 'ENOTDIR' // tslint:disable-line
      throw err
    }
  }
  const filePath = path.join(projectDir, 'package.json')
  return {
    fileName: 'package.json',
    manifest: null,
    writeProjectManifest: (manifest: ProjectManifest) => writeProjectManifest(filePath, manifest),
  }
}

export async function readExactProjectManifest (manifestPath: string) {
  const base = path.basename(manifestPath).toLowerCase()
  switch (base) {
    case 'package.json': {
      const { data, text } = await readJsonFile(manifestPath)
      const { indent } = detectIndent(text)
      return {
        manifest: data,
        writeProjectManifest: createManifestWriter({
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
        writeProjectManifest: createManifestWriter({
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
        writeProjectManifest: createManifestWriter({ initialManifest: manifest, manifestPath }),
      }
    }
  }
  throw new Error(`Not supported manifest name "${base}"`)
}

async function readPackageYaml (filePath: string) {
  try {
    return await readYamlFile<ProjectManifest>(filePath)
  } catch (err) {
    if (err.name !== 'YAMLException') throw err
    err.message += `\nin ${filePath}`
    err['code'] = 'ERR_PNPM_YAML_PARSE'
    throw err
  }
}

function createManifestWriter (
  opts: {
    initialManifest: ProjectManifest,
    indent?: string | number | undefined,
    manifestPath: string,
  },
): (WriteProjectManifest) {
  const initialManifest = normalize(JSON.parse(JSON.stringify(opts.initialManifest)))
  return async (updatedManifest: ProjectManifest, force?: boolean) => {
    updatedManifest = normalize(updatedManifest)
    if (force === true || !equal(initialManifest, updatedManifest)) {
      return writeProjectManifest(opts.manifestPath, updatedManifest, { indent: opts.indent })
    }
  }
}

const dependencyKeys = new Set([
  'dependencies',
  'devDependencies',
  'optionalDependencies',
  'peerDependencies',
])

function normalize (manifest: ProjectManifest) {
  const result = {}

  for (const key of Object.keys(manifest)) {
    if (!dependencyKeys.has(key)) {
      result[key] = manifest[key]
    } else if (Object.keys(manifest[key]).length !== 0) {
      result[key] = sortKeys(manifest[key])
    }
  }

  return result
}
