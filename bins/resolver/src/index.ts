import { existsSync } from 'node:fs'
import path from 'node:path'

import type { DependencyManifest, PackageBin } from '@pnpm/types'
import { isSubdir } from 'is-subdir'
import { glob } from 'tinyglobby'

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

function commandsFromBin (bin: PackageBin, pkgName: string, pkgPath: string): Command[] {
  const cmds: Command[] = []
  for (const [commandName, binRelativePath] of typeof bin === 'string' ? [[pkgName, bin]] : Object.entries(bin)) {
    const binName = commandName[0] === '@'
      ? commandName.slice(commandName.indexOf('/') + 1)
      : commandName
    // Validate: must be safe (no path traversal) - only allow URL-safe chars or $
    if (binName !== encodeURIComponent(binName) && binName !== '$') {
      continue
    }
    const binPath = resolveBinPath(pkgPath, binRelativePath)
    if (binPath == null) {
      continue
    }
    cmds.push({ name: binName, path: binPath })
  }
  return cmds
}

function resolveBinPath (pkgPath: string, binRelativePath: string): string | null {
  const directBinPath = path.join(pkgPath, binRelativePath)
  const directBinPathIsSafe = isSubdir(pkgPath, directBinPath)
  if (!referencesDependencyNodeModules(binRelativePath)) {
    return directBinPathIsSafe ? directBinPath : null
  }

  const virtualStoreBinPath = resolveBinPathFromNearestNodeModulesDir(pkgPath, binRelativePath)
  if (directBinPathIsSafe && existsSync(directBinPath)) {
    return directBinPath
  }
  return virtualStoreBinPath ?? (directBinPathIsSafe ? directBinPath : null)
}

function referencesDependencyNodeModules (binRelativePath: string): boolean {
  const normalizedBinRelativePath = binRelativePath.replace(/\\/g, '/')
  return normalizedBinRelativePath.startsWith('node_modules/') || normalizedBinRelativePath.startsWith('./node_modules/')
}

function resolveBinPathFromNearestNodeModulesDir (pkgPath: string, binRelativePath: string): string | null {
  const nearestNodeModulesDir = findNearestNodeModulesDir(pkgPath)
  if (nearestNodeModulesDir == null) return null

  const normalizedBinRelativePath = binRelativePath.replace(/\\/g, '/')
  const relativePathFromNodeModules = normalizedBinRelativePath
    .replace(/^\.\//, '')
    .slice('node_modules/'.length)

  const binPath = path.join(nearestNodeModulesDir, relativePathFromNodeModules)
  return isSubdir(nearestNodeModulesDir, binPath) ? binPath : null
}

function findNearestNodeModulesDir (pkgPath: string): string | null {
  let currentDir = path.dirname(pkgPath)
  while (currentDir !== path.dirname(currentDir)) {
    if (path.basename(currentDir) === 'node_modules') {
      return currentDir
    }
    currentDir = path.dirname(currentDir)
  }
  return null
}
