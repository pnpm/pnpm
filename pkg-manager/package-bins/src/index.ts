import '@total-typescript/ts-reset'

import path from 'node:path'

import fastGlob from 'fast-glob'
import isSubdir from 'is-subdir'

import { BundledManifest } from '@pnpm/store-controller-types'
import type { Command, ProjectManifest, PackageBin } from '@pnpm/types'

export async function getBinsFromPackageManifest(
  manifest: ProjectManifest | BundledManifest,
  pkgPath: string
): Promise<Command[]> {
  if (manifest.bin) {
    return commandsFromBin(manifest.bin, manifest.name, pkgPath)
  }

  if (manifest.directories?.bin) {
    const binDir = path.join(pkgPath, manifest.directories.bin)

    const files = await findFiles(binDir)

    return files.map((file) => ({
      name: path.basename(file),
      path: path.join(binDir, file),
    }))
  }

  return []
}

async function findFiles(dir: string): Promise<string[]> {
  try {
    return await fastGlob('**', {
      cwd: dir,
      onlyFiles: true,
      followSymbolicLinks: false,
    })
  } catch (err: unknown) {
    if ((err as NodeJS.ErrnoException).code !== 'ENOENT') {
      throw err
    }

    return []
  }
}

function commandsFromBin(bin: PackageBin, pkgName: string | undefined, pkgPath: string): {
  name: string;
  path: string;
}[] {
  if (typeof bin === 'string' && typeof pkgName === 'string') {
    return [
      {
        name: normalizeBinName(pkgName),
        path: path.join(pkgPath, bin),
      },
    ]
  }

  return Object.keys(bin)
    .filter(
      (commandName: string): boolean => {
        return encodeURIComponent(commandName) === commandName ||
          commandName === '$' ||
          commandName.startsWith('@');
      }
    )
    .map((commandName: string): {
      name: string;
      path: string;
    } => {
      return {
        name: normalizeBinName(commandName),
        // @ts-ignore
        path: path.join(pkgPath, bin[commandName]),
      };
    })
    .filter((cmd: {
      name: string;
      path: string;
    }): boolean => {
      return isSubdir(pkgPath, cmd.path);
    })
}

function normalizeBinName(name: string) {
  return name.startsWith('@') ? name.slice(name.indexOf('/') + 1) : name
}
