import fs from 'node:fs/promises'
import { createRequire } from 'node:module'
import path from 'node:path'

import type { DependencyManifest, PackageBin } from '@pnpm/types'
import { isSubdir } from 'is-subdir'
import { glob } from 'tinyglobby'

const require = createRequire(import.meta.dirname)

export interface Command {
  name: string
  path: string
}

// Maps a bin name to all packages that are legitimate owners of it, beyond
// the default rule that a package named `X` owns the `X` bin.  For example,
// `npx` ships inside the `npm` package, and `pnpx` ships inside both the
// `pnpm` package and the `@pnpm/exe` package.
export const BIN_OWNER_OVERRIDES: Record<string, string[]> = {
  npx: ['npm'],
  pn: ['pnpm', '@pnpm/exe'],
  pnpm: ['@pnpm/exe'],
  pnpx: ['pnpm', '@pnpm/exe'],
  pnx: ['pnpm', '@pnpm/exe'],
}

export function pkgOwnsBin (binName: string, pkgName: string): boolean {
  return binName === pkgName || BIN_OWNER_OVERRIDES[binName]?.includes(pkgName) === true
}

export async function getBinsFromPackageManifest (manifest: DependencyManifest, pkgPath: string): Promise<Command[]> {
  if (manifest.bin) {
    return commandsFromBin(manifest.bin, manifest.name, pkgPath)
  }
  if (manifest.directories?.bin) {
    const binDir = path.join(pkgPath, manifest.directories.bin)
    // Validate: directories.bin must be within the package root
    if (!isSubdir(pkgPath, binDir)) {
      return []
    }
    const files = await findFiles(binDir)
    return files.map((file) => ({
      name: path.basename(file),
      path: path.join(binDir, file),
    }))
  }
  return []
}

async function findFiles (dir: string): Promise<string[]> {
  try {
    return await glob('**', {
      cwd: dir,
      onlyFiles: true,
      followSymbolicLinks: false,
      expandDirectories: false,
    })
  } catch (err: any) { // eslint-disable-line
    if ((err as NodeJS.ErrnoException).code !== 'ENOENT') {
      throw err
    }
    return []
  }
}

async function commandsFromBin (bin: PackageBin, pkgName: string, pkgPath: string): Promise<Command[]> {
  const cmds = await Promise.all((typeof bin === 'string' ? [[pkgName, bin]] : Object.entries(bin)).map(async ([commandName, binRelativePath]) => {
    const binName = commandName[0] === '@'
      ? commandName.slice(commandName.indexOf('/') + 1)
      : commandName
    // Validate: must be safe (no path traversal) - only allow URL-safe chars or $
    if (binName !== encodeURIComponent(binName) && binName !== '$') {
      return null
    }
    const binPath = await resolveBinPath(binRelativePath, pkgPath)
    if (binPath == null) {
      return null
    }
    return { name: binName, path: binPath }
  }))
  return cmds.filter((cmd): cmd is Command => cmd != null)
}

async function resolveBinPath (binRelativePath: string, pkgPath: string): Promise<string | null> {
  const binPath = path.join(pkgPath, binRelativePath)
  if (!isSubdir(pkgPath, binPath)) {
    return null
  }
  const nodeModulesSpecifier = getNodeModulesSpecifier(binRelativePath)
  if (nodeModulesSpecifier != null) {
    const resolveFromPath = await fs.realpath(pkgPath).catch(() => pkgPath)
    try {
      return require.resolve(nodeModulesSpecifier, { paths: [resolveFromPath] })
    } catch {}
  }
  return binPath
}

function getNodeModulesSpecifier (binRelativePath: string): string | null {
  const pathParts = path.normalize(binRelativePath).split(path.sep).filter(Boolean)
  while (pathParts[0] === '.') {
    pathParts.shift()
  }
  if (pathParts[0] !== 'node_modules') {
    return null
  }
  const specifierParts = pathParts.slice(1)
  if (
    specifierParts.length === 0 ||
    specifierParts.includes('.') ||
    specifierParts.includes('..') ||
    specifierParts[0].startsWith('.')
  ) {
    return null
  }
  if (specifierParts[0].startsWith('@') && specifierParts.length < 2) {
    return null
  }
  return specifierParts.join('/')
}
