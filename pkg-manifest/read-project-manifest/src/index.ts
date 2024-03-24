import '@total-typescript/ts-reset'

import path from 'node:path'
import { promises as fs, type Stats } from 'node:fs'

import isWindows from 'is-windows'
import equal from 'fast-deep-equal'
import cloneDeep from 'lodash.clonedeep'
import readYamlFile from 'read-yaml-file'

import {
  extractComments,
  type CommentSpecifier,
} from '@pnpm/text.comments-parser'
import { PnpmError } from '@pnpm/error'
import detectIndent from '@gwhitney/detect-indent'
import { writeProjectManifest } from '@pnpm/write-project-manifest'
import type { ProjectManifest, WriteProjectManifest } from '@pnpm/types'

import { readJson5File, readJsonFile } from './readFile.js'

export async function safeReadProjectManifestOnly(projectDir: string): Promise<ProjectManifest | undefined> {
  try {
    return await readProjectManifestOnly(projectDir)
  } catch (err: unknown) {
    if (
      (err as NodeJS.ErrnoException).code ===
      'ERR_PNPM_NO_IMPORTER_MANIFEST_FOUND'
    ) {
      return undefined
    }

    throw err
  }
}

export async function readProjectManifest(projectDir: string): Promise<{
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

  throw new PnpmError(
    'NO_IMPORTER_MANIFEST_FOUND',
    `No package.json (or package.yaml, or package.json5) was found in "${projectDir}".`
  )
}

export async function readProjectManifestOnly(
  projectDir: string
): Promise<ProjectManifest> {
  const { manifest } = await readProjectManifest(projectDir)

  return manifest
}

export async function tryReadProjectManifest(projectDir: string): Promise<{
  fileName: string
  manifest: ProjectManifest | undefined
  writeProjectManifest: WriteProjectManifest
}> {
  try {
    const manifestPath = path.join(projectDir, 'package.json')

    const { data, text } = await readJsonFile(manifestPath)

    return {
      fileName: 'package.json',
      manifest: data,
      writeProjectManifest: createManifestWriter({
        ...detectFileFormatting(text),
        initialManifest: data,
        manifestPath,
      }),
    }
  } catch (err: unknown) {
    // @ts-ignore
    if (err.code !== 'ENOENT') {
      throw err
    }
  }

  try {
    const manifestPath = path.join(projectDir, 'package.json5')

    const { data, text } = await readJson5File(manifestPath)

    return {
      fileName: 'package.json5',
      manifest: data,
      writeProjectManifest: createManifestWriter({
        ...detectFileFormattingAndComments(text),
        initialManifest: data,
        manifestPath,
      }),
    }
  } catch (err: unknown) {
    // @ts-ignore
    if (err.code !== 'ENOENT') {
      throw err
    }
  }

  try {
    const manifestPath = path.join(projectDir, 'package.yaml')

    const manifest = await readPackageYaml(manifestPath)

    return {
      fileName: 'package.yaml',
      manifest,
      writeProjectManifest: createManifestWriter({
        initialManifest: manifest,
        manifestPath,
      }),
    }
  } catch (err: unknown) {
    // @ts-ignore
    if (err.code !== 'ENOENT') {
      throw err
    }
  }

  if (isWindows()) {
    // ENOTDIR isn't used on Windows, but pnpm expects it.
    let s: Stats | undefined

    try {
      s = await fs.stat(projectDir)
    } catch (err: unknown) {
      // Ignore
    }

    if (s != null && !s.isDirectory()) {
      const err = new Error(`"${projectDir}" is not a directory`)
      // @ts-expect-error
      err.code = 'ENOTDIR'
      throw err
    }
  }

  const filePath = path.join(projectDir, 'package.json')

  return {
    fileName: 'package.json',
    manifest: undefined,
    writeProjectManifest: async (manifest: ProjectManifest) =>
      writeProjectManifest(filePath, manifest),
  }
}

function detectFileFormattingAndComments(text: string): {
  comments: CommentSpecifier[] | undefined;
  indent: string;
  insertFinalNewline: boolean;
} {
  const { comments, text: newText, hasFinalNewline } = extractComments(text)

  return {
    comments,
    indent: detectIndent.default(newText).indent,
    insertFinalNewline: hasFinalNewline,
  }
}

function detectFileFormatting(text: string): {
  indent: string;
  insertFinalNewline: boolean;
} {
  return {
    indent: detectIndent.default(text).indent,
    insertFinalNewline: text.endsWith('\n'),
  }
}

export async function readExactProjectManifest(manifestPath: string): Promise<{
  manifest: ProjectManifest;
  writeProjectManifest: WriteProjectManifest;
}> {
  const base = path.basename(manifestPath).toLowerCase()

  switch (base) {
    case 'package.json': {
      const { data, text } = await readJsonFile(manifestPath)

      return {
        manifest: data,
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
        manifest: data,
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
        manifest,
        writeProjectManifest: createManifestWriter({
          initialManifest: manifest,
          manifestPath,
        }),
      }
    }
  }

  throw new Error(`Not supported manifest name "${base}"`)
}

async function readPackageYaml(filePath: string) {
  try {
    return await readYamlFile.default<ProjectManifest>(filePath)
  } catch (err: unknown) {
    // @ts-ignore
    if (err.name !== 'YAMLException') {
      throw err
    }
    // @ts-ignore
    err.message = `${err.message as string}\nin ${filePath}`
    // @ts-ignore
    err.code = 'ERR_PNPM_YAML_PARSE'
    throw err
  }
}

function createManifestWriter(opts: {
  initialManifest: ProjectManifest
  comments?: CommentSpecifier[] | undefined
  indent?: string | number | undefined
  insertFinalNewline?: boolean
  manifestPath: string
}): WriteProjectManifest {
  let initialManifest = normalize(opts.initialManifest)
  return async (updatedManifest: ProjectManifest, force?: boolean) => {
    updatedManifest = normalize(updatedManifest)
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

const dependencyKeys = new Set([
  'dependencies',
  'devDependencies',
  'optionalDependencies',
  'peerDependencies',
])

function normalize(manifest: ProjectManifest) {
  const result: Record<string, unknown> = {}
  for (const key in manifest) {
    if (Object.prototype.hasOwnProperty.call(manifest, key)) {
      const value = manifest[key as keyof ProjectManifest]
      if (typeof value !== 'object' || !dependencyKeys.has(key)) {
        result[key] = cloneDeep(value)
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
