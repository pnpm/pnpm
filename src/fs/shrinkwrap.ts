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

export function addToShrinkwrap(
  shrinkwrap: Shrinkwrap,
  spec: PackageSpec,
  resolution: Resolution
): void {
  switch (resolution.type) {
    case 'package':
      resolution = {...resolution}
      delete resolution.pkg
      shrinkwrap[spec.raw] = resolution
      break;
    case 'tarball':
    case 'git-repo':
      shrinkwrap[spec.raw] = {...resolution}
      break;
  }
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
