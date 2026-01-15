import path from 'path'
import { type DependencyManifest, type PackageBin } from '@pnpm/types'
import { glob } from 'tinyglobby'
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
    const binPath = path.join(pkgPath, binRelativePath)
    if (!isSubdir(pkgPath, binPath)) {
      continue
    }
    cmds.push({ name: binName, path: binPath })
  }
  return cmds
}
