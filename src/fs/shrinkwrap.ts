import path = require('path')
import pnpmPkgJson from '../pnpmPkgJson'
import {
  read as readYaml,
  write as writeYaml
} from './yamlfs'
import {PackageSpec, Resolution} from '../resolve'

const shrinkwrapFilename = 'shrinkwrap.yaml'

export type Shrinkwrap = {
  [dependency: string]: Resolution
}

export async function read (pkgPath: string): Promise<Shrinkwrap | null> {
  const shrinkwrapPath = path.join(pkgPath, shrinkwrapFilename)
  try {
    return await readYaml<Shrinkwrap>(shrinkwrapPath)
  } catch (err) {
    if ((<NodeJS.ErrnoException>err).code !== 'ENOENT') {
      throw err
    }
    return null
  }
}

export function save (pkgPath: string, shrinkwrap: Shrinkwrap) {
  const shrinkwrapPath = path.join(pkgPath, shrinkwrapFilename)
  return writeYaml(shrinkwrapPath, shrinkwrap)
}
