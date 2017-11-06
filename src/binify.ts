import {PackageJson, PackageBin} from '@pnpm/types'
import path = require('path')
import {Stats} from 'fs'
import fs = require('mz/fs')
import pFilter = require('p-filter')

export type Command = {
  name: string,
  path: string,
}

/**
 * Returns bins for a package in a standard object format. This normalizes
 * between npm's string and object formats.
 *
 * @example
 *    binify({ name: 'rimraf', bin: 'cmd.js' })
 *    => { rimraf: 'cmd.js' }
 *
 *    binify({ name: 'rmrf', bin: { rmrf: 'cmd.js' } })
 *    => { rmrf: 'cmd.js' }
 */
export default async function binify (pkg: PackageJson, pkgPath: string): Promise<Command[]> {
  if (pkg.bin) {
    return commandsFromBin(pkg.bin, pkg.name, pkgPath)
  }
  if (pkg.directories && pkg.directories.bin) {
    const binDir = path.join(pkgPath, pkg.directories.bin)
    const files = await findFiles(binDir)
    return pFilter(
      files.map(file => ({
        name: file,
        path: path.join(binDir, file)
      })),
      async (cmd: Command) => (await fs.stat(cmd.path)).isFile()
    )
  }
  return []
}

async function findFiles (dir: string): Promise<string[]> {
  try {
    return await fs.readdir(dir)
  } catch (err) {
    if ((<NodeJS.ErrnoException>err).code !== 'ENOENT') {
      throw err
    }
    return []
  }
}

function commandsFromBin (bin: PackageBin, pkgName: string, pkgPath: string) {
  if (typeof bin === 'string') {
    return [
      {
        name: pkgName,
        path: path.join(pkgPath, bin),
      },
    ]
  }
  return Object.keys(bin)
    .map(commandName => ({
      name: commandName,
      path: path.join(pkgPath, bin[commandName])
    }))
}
