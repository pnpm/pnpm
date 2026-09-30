import path from 'node:path'

import { type CommentSpecifier, extractComments } from '@pnpm/text.comments-parser'
import type { ProjectManifest } from '@pnpm/types'
import { writeProjectManifest } from '@pnpm/workspace.project-manifest-writer'
import detectIndent from 'detect-indent'
import equal from 'fast-deep-equal'
import { readYamlFile, readYamlFileSync } from 'read-yaml-file'

import { convertManifestAfterRead, convertManifestBeforeWrite } from './manifestRuntime.js'
import {
  readJson5File,
  readJson5FileSync,
  readJsonFile,
  readJsonFileSync,
} from './readFile.js'

export type WriteProjectManifest = (manifest: ProjectManifest, force?: boolean) => Promise<void>

export interface ReadExactProjectManifestResult {
  manifest: ProjectManifest
  writeProjectManifest: WriteProjectManifest
}

export async function readExactProjectManifest (manifestPath: string): Promise<ReadExactProjectManifestResult> {
  const base = path.basename(manifestPath).toLowerCase()
  switch (base) {
    case 'package.json':
      return readExactPackageJson(manifestPath)
    case 'package.json5':
      return readExactPackageJson5(manifestPath)
    case 'package.yaml':
      return readExactPackageYaml(manifestPath)
  }
  throw new Error(`Not supported manifest name "${base}"`)
}

export function readExactProjectManifestSync (manifestPath: string): ReadExactProjectManifestResult {
  const base = path.basename(manifestPath).toLowerCase()
  switch (base) {
    case 'package.json':
      return readExactPackageJsonSync(manifestPath)
    case 'package.json5':
      return readExactPackageJson5Sync(manifestPath)
    case 'package.yaml':
      return readExactPackageYamlSync(manifestPath)
  }
  throw new Error(`Not supported manifest name "${base}"`)
}

export async function tryReadProjectManifestFromDir (projectDir: string): Promise<{
  fileName: string
  manifest: ProjectManifest
  writeProjectManifest: WriteProjectManifest
} | undefined> {
  const jsonResult = await tryReadPackageJson(projectDir)
  if (jsonResult) return jsonResult

  const json5Result = await tryReadPackageJson5(projectDir)
  if (json5Result) return json5Result

  return tryReadPackageYaml(projectDir)
}

async function tryReadPackageJson (projectDir: string) {
  try {
    const manifestPath = path.join(projectDir, 'package.json')
    const result = await readExactPackageJson(manifestPath)
    return { fileName: 'package.json', ...result }
  } catch (err: any) { // eslint-disable-line
    if (err.code !== 'ENOENT') throw err
    return undefined
  }
}

async function tryReadPackageJson5 (projectDir: string) {
  try {
    const manifestPath = path.join(projectDir, 'package.json5')
    const result = await readExactPackageJson5(manifestPath)
    return { fileName: 'package.json5', ...result }
  } catch (err: any) { // eslint-disable-line
    if (err.code !== 'ENOENT') throw err
    return undefined
  }
}

async function tryReadPackageYaml (projectDir: string) {
  try {
    const manifestPath = path.join(projectDir, 'package.yaml')
    const result = await readExactPackageYaml(manifestPath)
    return { fileName: 'package.yaml', ...result }
  } catch (err: any) { // eslint-disable-line
    if (err.code !== 'ENOENT') throw err
    return undefined
  }
}

async function readExactPackageJson (manifestPath: string): Promise<ReadExactProjectManifestResult> {
  const { data, text } = await readJsonFile(manifestPath)
  return createManifestResult(data, manifestPath, detectFileFormatting(text))
}

function readExactPackageJsonSync (manifestPath: string): ReadExactProjectManifestResult {
  const { data, text } = readJsonFileSync(manifestPath)
  return createManifestResult(data, manifestPath, detectFileFormatting(text))
}

async function readExactPackageJson5 (manifestPath: string): Promise<ReadExactProjectManifestResult> {
  const { data, text } = await readJson5File(manifestPath)
  return createManifestResult(data, manifestPath, detectFileFormattingAndComments(text))
}

function readExactPackageJson5Sync (manifestPath: string): ReadExactProjectManifestResult {
  const { data, text } = readJson5FileSync(manifestPath)
  return createManifestResult(data, manifestPath, detectFileFormattingAndComments(text))
}

async function readExactPackageYaml (manifestPath: string): Promise<ReadExactProjectManifestResult> {
  const manifest = await readPackageYaml(manifestPath)
  return createManifestResult(manifest, manifestPath)
}

function readExactPackageYamlSync (manifestPath: string): ReadExactProjectManifestResult {
  const manifest = readPackageYamlSync(manifestPath)
  return createManifestResult(manifest, manifestPath)
}

function createManifestResult (
  data: ProjectManifest,
  manifestPath: string,
  formatting?: FileFormattingAndComments | FileFormatting
): ReadExactProjectManifestResult {
  const emptyDependencyFields = findEmptyDependencyFields(data)
  return {
    manifest: convertManifestAfterRead(data),
    writeProjectManifest: createManifestWriter({
      ...formatting,
      emptyDependencyFields,
      initialManifest: data,
      manifestPath,
    }),
  }
}

interface FileFormatting {
  crlf: boolean
  indent: string
  insertFinalNewline: boolean
}

interface FileFormattingAndComments extends FileFormatting {
  comments?: CommentSpecifier[]
}

function detectFileFormatting (text: string): FileFormatting {
  return {
    crlf: text.includes('\r\n'),
    indent: detectIndent(text).indent,
    insertFinalNewline: text.endsWith('\n'),
  }
}

function detectFileFormattingAndComments (text: string): FileFormattingAndComments {
  const { comments, text: newText, hasFinalNewline } = extractComments(text)
  return {
    comments,
    crlf: text.includes('\r\n'),
    indent: detectIndent(newText).indent,
    insertFinalNewline: hasFinalNewline,
  }
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

function readPackageYamlSync (filePath: string): ProjectManifest {
  try {
    return readYamlFileSync<ProjectManifest>(filePath)
  } catch (err: any) { // eslint-disable-line
    if (err.name !== 'YAMLException') throw err
    err.message = `${err.message as string}\nin ${filePath}`
    err.code = 'ERR_PNPM_YAML_PARSE'
    throw err
  }
}

export function createManifestWriter (
  opts: {
    initialManifest: ProjectManifest
    emptyDependencyFields?: ReadonlySet<string>
    comments?: CommentSpecifier[]
    crlf?: boolean
    indent?: string | number | undefined
    insertFinalNewline?: boolean
    manifestPath: string
  }
): WriteProjectManifest {
  let emptyDependencyFields = opts.emptyDependencyFields ?? findEmptyDependencyFields(opts.initialManifest)
  let initialManifest = normalize(opts.initialManifest, emptyDependencyFields)
  return async (updatedManifest: ProjectManifest, force?: boolean) => {
    updatedManifest = convertManifestBeforeWrite(normalize(updatedManifest, emptyDependencyFields))
    if (force === true || !equal(initialManifest, updatedManifest)) {
      await writeProjectManifest(opts.manifestPath, updatedManifest, {
        comments: opts.comments,
        crlf: opts.crlf,
        indent: opts.indent,
        insertFinalNewline: opts.insertFinalNewline,
      })
      emptyDependencyFields = findEmptyDependencyFields(updatedManifest)
      initialManifest = normalize(updatedManifest, emptyDependencyFields)
    }
  }
}

const DEPENDENCY_KEYS = new Set([
  'dependencies',
  'devDependencies',
  'optionalDependencies',
  'peerDependencies',
])

export function findEmptyDependencyFields (manifest: ProjectManifest): Set<string> {
  const fields = new Set<string>()
  for (const key of DEPENDENCY_KEYS) {
    if (isEmptyDependencyObject(manifest[key as keyof ProjectManifest])) {
      fields.add(key)
    }
  }
  return fields
}

function isEmptyDependencyObject (value: unknown): boolean {
  return typeof value === 'object' &&
    value !== null &&
    !Array.isArray(value) &&
    Object.keys(value).length === 0
}

function normalize (manifest: ProjectManifest, keepEmptyDependencyFields: ReadonlySet<string>): ProjectManifest {
  const result: Record<string, unknown> = {}
  for (const key of Object.keys(manifest)) {
    const value = manifest[key as keyof ProjectManifest]
    if (isDependencyObject(value, key)) {
      const keys = Object.keys(value)
      if (keys.length !== 0) {
        result[key] = sortObjectKeys(value, keys.sort())
      } else if (keepEmptyDependencyFields.has(key)) {
        result[key] = {}
      }
    } else {
      result[key] = structuredClone(value)
    }
  }
  return result
}

function isDependencyObject (value: unknown, key: string): value is Record<string, unknown> {
  return typeof value === 'object' &&
    value !== null &&
    DEPENDENCY_KEYS.has(key) &&
    !Array.isArray(value)
}

function sortObjectKeys (source: Record<string, unknown>, sortedKeys: string[]): Record<string, unknown> {
  const sorted: Record<string, unknown> = {}
  for (const fieldName of sortedKeys) {
    sorted[fieldName] = source[fieldName]
  }
  return sorted
}
