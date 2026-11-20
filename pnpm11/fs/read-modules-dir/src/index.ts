import type { Dirent } from 'node:fs'
import path from 'node:path'
import util from 'node:util'

import { isError } from '@pnpm/error'
import gracefulFs from 'graceful-fs'

const readdir = util.promisify(gracefulFs.readdir)

export async function readModulesDir (modulesDir: string): Promise<string[] | null> {
  try {
    return await _readModulesDir(modulesDir)
  } catch (err: unknown) {
    if (isError(err) && 'code' in err && err.code === 'ENOENT') return null
    throw err
  }
}

async function _readModulesDir (
  modulesDir: string,
  scope?: string
): Promise<string[]> {
  const pkgNames: string[] = []
  const parentDir = scope ? path.join(modulesDir, scope) : modulesDir
  const entries = await readdir(parentDir, { withFileTypes: true })
  await Promise.all(entries.map(async (entry) => {
    const names = await readDirEntry(entry, modulesDir, scope)
    if (names.length > 0) pkgNames.push(...names)
  }))
  return pkgNames
}

async function readDirEntry (
  entry: Dirent,
  modulesDir: string,
  scope?: string
): Promise<string[]> {
  if (entry.isFile() || entry.name[0] === '.') return []

  if (!scope && entry.name[0] === '@') {
    // Names below a symlinked scope container reach their target through the
    // symlink, wherever it points — a caller that deletes what it enumerates
    // follows it out of `modulesDir`. pnpm only ever symlinks the packages
    // inside a scope, never the scope itself, so skipping costs nothing.
    if (entry.isSymbolicLink()) return []
    return _readModulesDir(modulesDir, entry.name)
  }

  const pkgName = scope ? `${scope}/${entry.name}` : entry.name
  return [pkgName]
}
