import { promises as fs, type Stats } from 'node:fs'

import { PnpmError } from '@pnpm/error'
import type { CommentSpecifier } from '@pnpm/text.comments-parser'
import type { ProjectManifest } from '@pnpm/types'
import { writeProjectManifest } from '@pnpm/workspace.project-manifest-writer'
import equal from 'fast-deep-equal'
import isWindows from 'is-windows'

import { tryReadRawProjectManifestFromDir } from './manifestExactReader.js'

export type RawProjectManifest = Record<string, unknown>

export type WriteRawProjectManifest = (manifest: RawProjectManifest, force?: boolean) => Promise<void>

export interface ReadRawProjectManifestResult {
  fileName: string
  manifest: RawProjectManifest
  writeProjectManifest: WriteRawProjectManifest
}

export async function readRawProjectManifest (projectDir: string): Promise<ReadRawProjectManifestResult> {
  const found = await tryReadRawProjectManifestFromDir(projectDir)
  if (!found) {
    await assertDirectoryExistsOnWindows(projectDir)
    throw new PnpmError('NO_IMPORTER_MANIFEST_FOUND',
      `No package.json (or package.yaml, or package.json5) was found in "${projectDir}".`)
  }
  const rawData = found.fileName === 'package.yaml' && found.data == null ? {} : found.data
  if (!isPlainObject(rawData)) {
    throw invalidManifestRootError(found.fileName, found.manifestPath)
  }
  return {
    fileName: found.fileName,
    manifest: structuredClone(rawData),
    writeProjectManifest: createRawManifestWriter({
      ...found.formatting,
      fileName: found.fileName,
      initialManifest: rawData,
      manifestPath: found.manifestPath,
    }),
  }
}

export async function readRawProjectManifestOnly (projectDir: string): Promise<RawProjectManifest> {
  const { manifest } = await readRawProjectManifest(projectDir)
  return manifest
}

export async function assertDirectoryExistsOnWindows (projectDir: string): Promise<void> {
  if (!isWindows()) return

  // ENOTDIR isn't used on Windows, but pnpm expects it.
  let projectDirStats: Stats | undefined
  try {
    projectDirStats = await fs.stat(projectDir)
  } catch (err: any) { // eslint-disable-line
    // Ignore
  }
  if ((projectDirStats != null) && !projectDirStats.isDirectory()) {
    throw Object.assign(new Error(`"${projectDir}" is not a directory`), { code: 'ENOTDIR' })
  }
}

function createRawManifestWriter (
  opts: {
    comments?: CommentSpecifier[]
    crlf?: boolean
    fileName: string
    indent?: string | number | undefined
    initialManifest: RawProjectManifest
    insertFinalNewline?: boolean
    manifestPath: string
  }
): WriteRawProjectManifest {
  let initialManifest = structuredClone(opts.initialManifest)
  return async (updatedManifest: RawProjectManifest, force?: boolean) => {
    if (!isPlainObject(updatedManifest)) {
      throw invalidManifestRootError(opts.fileName, opts.manifestPath)
    }
    if (force === true || !valuesIdentical(initialManifest, updatedManifest)) {
      await writeProjectManifest(opts.manifestPath, updatedManifest as ProjectManifest, {
        comments: opts.comments,
        crlf: opts.crlf,
        indent: opts.indent,
        insertFinalNewline: opts.insertFinalNewline,
      })
      initialManifest = structuredClone(updatedManifest)
    }
  }
}

function invalidManifestRootError (fileName: string, manifestPath: string): PnpmError {
  const code = fileName === 'package.yaml' ? 'PACKAGE_MANIFEST_INVALID_ATTRIBUTE' : 'INVALID_MANIFEST'
  return new PnpmError(code, `${manifestPath}: the manifest root must be an object`)
}

function valuesIdentical (left: unknown, right: unknown): boolean {
  if (Object.is(left, right)) return true
  if (Array.isArray(left) && Array.isArray(right)) {
    return arraysIdentical(left, right)
  }
  if (isPlainObject(left) && isPlainObject(right)) {
    return plainObjectsIdentical(left, right)
  }
  return equal(left, right)
}

function arraysIdentical (left: unknown[], right: unknown[]): boolean {
  return left.length === right.length && left.every((item, index) => valuesIdentical(item, right[index]))
}

function plainObjectsIdentical (left: Record<string, unknown>, right: Record<string, unknown>): boolean {
  const leftKeys = Object.keys(left)
  const rightKeys = Object.keys(right)
  if (leftKeys.length !== rightKeys.length) return false
  return leftKeys.every((key, index) => key === rightKeys[index] && valuesIdentical(left[key], right[key]))
}

function isPlainObject (value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}
