import path from 'path'
import { type DependencyManifest, type PackageBin } from '@pnpm/types'
import fastGlob from 'fast-glob'
import isSubdir from 'is-subdir'

export interface Command {
  name: string
  path: string
}

export async function getBinsFromPackageManifest (manifest: DependencyManifest, pkgPath: string): Promise<Command[]> {
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

async function findFiles (dir: string): Promise<string[]> {
  try {
    return await fastGlob('**', {
      cwd: dir,
      onlyFiles: true,
      followSymbolicLinks: false,
    })
  } catch (err: any) { // eslint-disable-line
    if ((err as NodeJS.ErrnoException).code !== 'ENOENT') {
      throw err
    }
    return []
  }
}

function commandsFromBin (bin: PackageBin, pkgName: string, pkgPath: string) {
  if (typeof bin === 'string') {
    return [
      {
        name: normalizeBinName(pkgName),
        path: path.join(pkgPath, bin),
      },
    ]
  }
  return Object.keys(bin)
    .filter((commandName) => encodeURIComponent(commandName) === commandName || commandName === '$' || commandName[0] === '@')
    .map((commandName) => ({
      name: normalizeBinName(commandName),
      path: path.join(pkgPath, bin[commandName]),
    }))
    .filter((cmd) => isSubdir(pkgPath, cmd.path))
}

function normalizeBinName (name: string) {
  return name[0] === '@' ? name.slice(name.indexOf('/') + 1) : name
}
