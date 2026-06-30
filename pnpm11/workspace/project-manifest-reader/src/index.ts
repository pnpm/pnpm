import { promises as fs, type Stats } from 'node:fs'
import path from 'node:path'

import { PnpmError } from '@pnpm/error'
import { convertEnginesRuntimeToDependencies } from '@pnpm/pkg-manifest.utils'
import { type CommentSpecifier, extractComments } from '@pnpm/text.comments-parser'
import type { EngineDependency, ProjectManifest } from '@pnpm/types'
import { writeProjectManifest } from '@pnpm/workspace.project-manifest-writer'
import detectIndent from 'detect-indent'
import equal from 'fast-deep-equal'
import isWindows from 'is-windows'
import pLimit from 'p-limit'
import { readYamlFile } from 'read-yaml-file'

import {
  readJson5File,
  readJsonFile,
} from './readFile.js'

export type WriteProjectManifest = (manifest: ProjectManifest, force?: boolean) => Promise<void>

const limitProjectManifestReads = pLimit(4)

export async function safeReadProjectManifestOnly (projectDir: string): Promise<ProjectManifest | null> {
  return limitProjectManifestReads(async () => {
    try {
      return await readProjectManifestOnly(projectDir)
    } catch (err: any) { // eslint-disable-line
      if ((err as NodeJS.ErrnoException).code === 'ERR_PNPM_NO_IMPORTER_MANIFEST_FOUND') {
        return null
      }
      throw err
    }
  })
}

export async function readProjectManifest (projectDir: string): Promise<{
  fileName: string
  manifest: ProjectManifest
  writeProjectManifest: WriteProjectManifest
}> {
  const result = await tryReadProjectManifest(projectDir)
  if (result.manifest !== null) {
    return result as {
      fileName: string
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
  fileName: string
  manifest: ProjectManifest | null
  writeProjectManifest: WriteProjectManifest
}> {
  try {
    const manifestPath = path.join(projectDir, 'package.json')
    const { data, text } = await readJsonFile(manifestPath)
    return {
      fileName: 'package.json',
      manifest: convertManifestAfterRead(data),
      writeProjectManifest: createManifestWriter({
        ...detectFileFormatting(text),
        initialManifest: data,
        manifestPath,
      }),
    }
  } catch (err: any) { // eslint-disable-line
    if (err.code !== 'ENOENT') throw err
  }
  try {
    const manifestPath = path.join(projectDir, 'package.json5')
    const { data, text } = await readJson5File(manifestPath)
    return {
      fileName: 'package.json5',
      manifest: convertManifestAfterRead(data),
      writeProjectManifest: createManifestWriter({
        ...detectFileFormattingAndComments(text),
        initialManifest: data,
        manifestPath,
      }),
    }
  } catch (err: any) { // eslint-disable-line
    if (err.code !== 'ENOENT') throw err
  }
  try {
    const manifestPath = path.join(projectDir, 'package.yaml')
    const manifest = await readPackageYaml(manifestPath)
    return {
      fileName: 'package.yaml',
      manifest: convertManifestAfterRead(manifest),
      writeProjectManifest: createManifestWriter({ initialManifest: manifest, manifestPath }),
    }
  } catch (err: any) { // eslint-disable-line
    if (err.code !== 'ENOENT') throw err
  }
  if (isWindows()) {
    // ENOTDIR isn't used on Windows, but pnpm expects it.
    let s: Stats | undefined
    try {
      s = await fs.stat(projectDir)
    } catch (err: any) { // eslint-disable-line
      // Ignore
    }
    if ((s != null) && !s.isDirectory()) {
      const err = new Error(`"${projectDir}" is not a directory`)
      // @ts-expect-error
      err['code'] = 'ENOTDIR'
      throw err
    }
  }
  const filePath = path.join(projectDir, 'package.json')
  return {
    fileName: 'package.json',
    manifest: null,
    writeProjectManifest: async (manifest: ProjectManifest) => writeProjectManifest(filePath, manifest),
  }
}

interface FileFormattingAndComments {
  comments?: CommentSpecifier[]
  indent: string
  insertFinalNewline: boolean
}

function detectFileFormattingAndComments (text: string): FileFormattingAndComments {
  const { comments, text: newText, hasFinalNewline } = extractComments(text)
  return {
    comments,
    indent: detectIndent(newText).indent,
    insertFinalNewline: hasFinalNewline,
  }
}

interface FileFormatting {
  indent: string
  insertFinalNewline: boolean
}

function detectFileFormatting (text: string): FileFormatting {
  return {
    indent: detectIndent(text).indent,
    insertFinalNewline: text.endsWith('\n'),
  }
}

interface ReadExactProjectManifestResult {
  manifest: ProjectManifest
  writeProjectManifest: WriteProjectManifest
}

export async function readExactProjectManifest (manifestPath: string): Promise<ReadExactProjectManifestResult> {
  const base = path.basename(manifestPath).toLowerCase()
  switch (base) {
    case 'package.json': {
      const { data, text } = await readJsonFile(manifestPath)
      return {
        manifest: convertManifestAfterRead(data),
        writeProjectManifest: createManifestWriter({
          ...detectFileFormatting(text),
          initialManifest: data,
          manifestPath,
        }),
      }
    }
    case 'package.json5': {
      const { data, text } = await readJson5File(manifestPath)
      return {
        manifest: convertManifestAfterRead(data),
        writeProjectManifest: createManifestWriter({
          ...detectFileFormattingAndComments(text),
          initialManifest: data,
          manifestPath,
        }),
      }
    }
    case 'package.yaml': {
      const manifest = await readPackageYaml(manifestPath)
      return {
        manifest: convertManifestAfterRead(manifest),
        writeProjectManifest: createManifestWriter({ initialManifest: manifest, manifestPath }),
      }
    }
  }
  throw new Error(`Not supported manifest name "${base}"`)
}

async function readPackageYaml (filePath: string): Promise<ProjectManifest> {
  try {
    return await readYamlFile<ProjectManifest>(filePath)
  } catch (err: any) { // eslint-disable-line
    if (err.name !== 'YAMLException') throw err
    err.message = `${err.message as string}\nin ${filePath}`
    err.code = 'ERR_PNPM_YAML_PARSE'
    throw err
  }
}

function createManifestWriter (
  opts: {
    initialManifest: ProjectManifest
    comments?: CommentSpecifier[]
    indent?: string | number | undefined
    insertFinalNewline?: boolean
    manifestPath: string
  }
): WriteProjectManifest {
  let initialManifest = normalize(opts.initialManifest)
  return async (updatedManifest: ProjectManifest, force?: boolean) => {
    updatedManifest = convertManifestBeforeWrite(normalize(updatedManifest))
    if (force === true || !equal(initialManifest, updatedManifest)) {
      await writeProjectManifest(opts.manifestPath, updatedManifest, {
        comments: opts.comments,
        indent: opts.indent,
        insertFinalNewline: opts.insertFinalNewline,
      })
      initialManifest = normalize(updatedManifest)
      return Promise.resolve(undefined)
    }
    return Promise.resolve(undefined)
  }
}

function convertManifestAfterRead (manifest: ProjectManifest): ProjectManifest {
  convertEnginesRuntimeToDependencies(manifest, 'devEngines', 'devDependencies')
  convertEnginesRuntimeToDependencies(manifest, 'engines', 'dependencies')
  return manifest
}

function convertManifestBeforeWrite (manifest: ProjectManifest): ProjectManifest {
  convertDependenciesToEnginesRuntime(manifest, 'devDependencies', 'devEngines')
  convertDependenciesToEnginesRuntime(manifest, 'dependencies', 'engines')
  return manifest
}

function convertDependenciesToEnginesRuntime (
  manifest: ProjectManifest,
  dependenciesFieldName: 'dependencies' | 'devDependencies',
  enginesFieldName: 'engines' | 'devEngines'
): void {
  const dependencies = readDependenciesField(manifest, dependenciesFieldName)
  for (const runtimeName of ['node', 'deno', 'bun']) {
    const dep = dependencies?.[runtimeName]
    if (dependencies != null && typeof dep === 'string' && dep.startsWith('runtime:')) {
      const version = dep.slice('runtime:'.length).trim()
      manifest[enginesFieldName] ??= {}

      const runtimeEntry: EngineDependency = {
        name: runtimeName,
        version,
        onFail: 'download',
      }

      const enginesField = manifest[enginesFieldName]!
      if (!enginesField.runtime) {
        enginesField.runtime = runtimeEntry
      } else if (Array.isArray(enginesField.runtime)) {
        const existing = enginesField.runtime.find(({ name }) => name === runtimeName)
        if (existing) {
          Object.assign(existing, runtimeEntry)
        } else {
          enginesField.runtime.push(runtimeEntry)
        }
      } else if (enginesField.runtime.name === runtimeName) {
        Object.assign(enginesField.runtime, runtimeEntry)
      } else {
        enginesField.runtime = [
          enginesField.runtime,
          runtimeEntry,
        ]
      }
      delete dependencies[runtimeName]
    } else {
      removeManagedRuntimeEntry(manifest[enginesFieldName], runtimeName)
    }
  }
}

function readDependenciesField (
  manifest: ProjectManifest,
  dependenciesFieldName: 'dependencies' | 'devDependencies'
): Record<string, unknown> | undefined {
  const dependencies = manifest[dependenciesFieldName] as unknown
  if (dependencies === undefined) return undefined
  if (dependencies === null || typeof dependencies !== 'object' || Array.isArray(dependencies)) {
    throw new PnpmError('INVALID_DEPENDENCIES_FIELD', `The "${dependenciesFieldName}" field must be an object.`)
  }
  return dependencies as Record<string, unknown>
}

function removeManagedRuntimeEntry (
  enginesField: ProjectManifest['devEngines'] | ProjectManifest['engines'],
  runtimeName: string
): void {
  if (!enginesField?.runtime) return

  if (Array.isArray(enginesField.runtime)) {
    const runtimes = enginesField.runtime.filter((runtime) => !isManagedRuntimeEntry(runtime, runtimeName))
    if (runtimes.length === 0) {
      delete enginesField.runtime
    } else {
      enginesField.runtime = runtimes
    }
  } else if (isManagedRuntimeEntry(enginesField.runtime, runtimeName)) {
    delete enginesField.runtime
  }
}

function isManagedRuntimeEntry (runtime: EngineDependency, runtimeName: string): boolean {
  return runtime.name === runtimeName &&
    runtime.onFail === 'download' &&
    typeof runtime.version === 'string'
}

const dependencyKeys = new Set([
  'dependencies',
  'devDependencies',
  'optionalDependencies',
  'peerDependencies',
])

function normalize (manifest: ProjectManifest): ProjectManifest {
  const result: Record<string, unknown> = {}
  for (const key in manifest) {
    if (Object.hasOwn(manifest, key)) {
      const value = manifest[key as keyof ProjectManifest]
      if (
        typeof value !== 'object' ||
        value === null ||
        !dependencyKeys.has(key) ||
        Array.isArray(value)
      ) {
        result[key] = structuredClone(value)
      } else {
        const keys = Object.keys(value)
        if (keys.length !== 0) {
          keys.sort()
          const sortedValue: Record<string, unknown> = {}
          for (const k of keys) {
            // @ts-expect-error this is fine
            sortedValue[k] = value[k]
          }
          result[key] = sortedValue
        }
      }
    }
  }

  return result
}
