import fs from 'node:fs'
import path from 'node:path'

import { safeReadPackageJsonFromDir } from '@pnpm/pkg-manifest.reader'
import type { DependencyManifest } from '@pnpm/types'

type FilesIndexArg = Map<string, unknown> | Record<string, unknown>

export function pkgRequiresBuild (manifest: Partial<DependencyManifest> | undefined, filesIndex: FilesIndexArg): boolean {
  return Boolean(
    manifest?.scripts != null && (
      Boolean(manifest.scripts.preinstall) ||
      Boolean(manifest.scripts.install) ||
      Boolean(manifest.scripts.postinstall)
    ) ||
    filesIncludeInstallScripts(filesIndex)
  )
}

function filesIncludeInstallScripts (filesIndex: FilesIndexArg): boolean {
  const keys = filesIndex instanceof Map ? filesIndex.keys() : Object.keys(filesIndex)
  for (const filename of keys) {
    if (filename === 'binding.gyp') {
      return true
    }
    if (filename.match(/^\.hooks[\\/]/) != null) {
      return true
    }
  }
  return false
}

/**
 * [`pkgRequiresBuild`] for a package that is already on disk.
 *
 * Reads the same two triggers off the directory: the manifest's install
 * scripts, and the presence of `binding.gyp` or `.hooks/`. Callers that hold
 * the package's files index should use [`pkgRequiresBuild`] instead - this is
 * for the ones that only have the extracted directory, such as a package whose
 * patch has just been applied.
 *
 * A directory that cannot be inspected - a missing or malformed manifest, an
 * unreadable entry - reports no build, matching the Rust `pkgRequiresBuild`.
 * There is no build to schedule for a package whose contents cannot be read,
 * and the lifecycle runner reports the manifest again when scripts do run.
 */
export async function dirRequiresBuild (dir: string): Promise<boolean> {
  const filesIndex = new Map<string, undefined>()
  if (fs.existsSync(path.join(dir, 'binding.gyp'))) {
    filesIndex.set('binding.gyp', undefined)
  }
  // Only a `.hooks` directory holds hooks, and only its entries are what
  // `filesIncludeInstallScripts` matches. A plain file by that name is not a
  // build trigger.
  if (dirEntryIsDirectory(path.join(dir, '.hooks'))) {
    filesIndex.set('.hooks/', undefined)
  }
  let manifest: Partial<DependencyManifest> | undefined
  try {
    manifest = await safeReadPackageJsonFromDir(dir) ?? undefined
  } catch {
    return false
  }
  return pkgRequiresBuild(manifest, filesIndex)
}

function dirEntryIsDirectory (entryPath: string): boolean {
  try {
    return fs.statSync(entryPath, { throwIfNoEntry: false })?.isDirectory() === true
  } catch {
    return false
  }
}
