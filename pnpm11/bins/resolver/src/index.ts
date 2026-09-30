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
  } catch (err: unknown) {
    if (typeof err === 'object' && err !== null && 'code' in err && (err as { code: unknown }).code === 'ENOENT') {
      return []
    }
    throw err
  }
}

function commandsFromBin (bin: PackageBin, pkgName: string, pkgPath: string): Command[] {
  const cmds: Command[] = []
  const binEntries = typeof bin === 'string' ? [[pkgName, bin]] : Object.entries(bin)
  for (const [commandName, binRelativePath] of binEntries) {
    const binName = resolveBinName(commandName)
    if (!isSafeBinName(binName)) continue
    const binPath = path.join(pkgPath, binRelativePath)
    if (!isSubdir(pkgPath, binPath)) continue
    cmds.push({ name: binName, path: binPath })
  }
  return cmds
}

function resolveBinName (commandName: string): string {
  return commandName[0] === '@'
    ? commandName.slice(commandName.indexOf('/') + 1)
    : commandName
}

function isSafeBinName (binName: string): boolean {
  if (binName === '' || binName === '.' || binName === '..') {
    return false
  }
  return binName === encodeURIComponent(binName) || binName === '$'
}
