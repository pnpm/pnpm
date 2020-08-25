import { promisify } from 'util'
import { DependencyManifest, PackageBin } from '@pnpm/types'
import { readdir, stat } from 'graceful-fs'
import path = require('path')
import isSubdir = require('is-subdir')
import pFilter = require('p-filter')

const readdirP = promisify(readdir)
const statP = promisify(stat)

export interface Command {
  name: string
  path: string
}

export default async function binify (manifest: DependencyManifest, pkgPath: string): Promise<Command[]> {
  if (manifest.bin) {
    return commandsFromBin(manifest.bin, manifest.name, pkgPath)
  }
  if (manifest.directories?.bin) {
    const binDir = path.join(pkgPath, manifest.directories.bin)
    const files = await findFiles(binDir)
    return pFilter(
      files.map((file) => ({
        name: file,
        path: path.join(binDir, file),
      })),
      async (cmd: Command) => (await statP(cmd.path)).isFile()
    )
  }
  return []
}

async function findFiles (dir: string): Promise<string[]> {
  try {
    return await readdirP(dir)
  } catch (err) {
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
        name: pkgName.startsWith('@') ? pkgName.substr(pkgName.indexOf('/') + 1) : pkgName,
        path: path.join(pkgPath, bin),
      },
    ]
  }
  return Object.keys(bin)
    .filter((commandName) => encodeURIComponent(commandName) === commandName)
    .map((commandName) => ({
      name: commandName,
      path: path.join(pkgPath, bin[commandName]),
    }))
    .filter((cmd) => isSubdir(pkgPath, cmd.path))
}
