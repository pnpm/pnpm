import { promises as fs, type Stats } from 'node:fs'
import path from 'node:path'

import { PnpmError } from '@pnpm/error'
import type { ProjectManifest } from '@pnpm/types'
import { writeProjectManifest } from '@pnpm/workspace.project-manifest-writer'
import isWindows from 'is-windows'
import pLimit from 'p-limit'

import {
  readExactProjectManifest,
  type ReadExactProjectManifestResult,
  readExactProjectManifestSync,
  tryReadProjectManifestFromDir,
  type WriteProjectManifest,
} from './manifestExactReader.js'

export type { ReadExactProjectManifestResult, WriteProjectManifest }
export { readExactProjectManifest, readExactProjectManifestSync }

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

export async function safeReadPublishManifest (projectDir: string): Promise<ProjectManifest | null> {
  return (await safeReadProjectManifestOnly(projectDir)) ?? safeReadParentPublishManifest(projectDir)
}

/**
 * Finds the manifest of a project whose `publishConfig.directory` is `publishDir`,
 * searching the ancestors of `publishDir`.
 */
export async function safeReadParentPublishManifest (publishDir: string): Promise<ProjectManifest | null> {
  const normalizedTarget = path.resolve(publishDir)
  let searchDir = path.dirname(normalizedTarget)
  while (true) {
    // eslint-disable-next-line no-await-in-loop -- the search climbs one ancestor at a time and stops at the first match
    const parentManifest = await safeReadProjectManifestOnly(searchDir)
    if (
      parentManifest?.publishConfig?.directory &&
      path.resolve(searchDir, parentManifest.publishConfig.directory) === normalizedTarget
    ) {
      return parentManifest
    }
    const next = path.dirname(searchDir)
    if (next === searchDir) return null
    searchDir = next
  }
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
  const found = await tryReadProjectManifestFromDir(projectDir)
  if (found) return found

  await assertDirectoryExistsOnWindows(projectDir)

  const filePath = path.join(projectDir, 'package.json')
  return {
    fileName: 'package.json',
    manifest: null,
    writeProjectManifest: async (manifest: ProjectManifest) => writeProjectManifest(filePath, manifest),
  }
}

async function assertDirectoryExistsOnWindows (projectDir: string): Promise<void> {
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
