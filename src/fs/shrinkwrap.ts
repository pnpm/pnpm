import path = require('path')
import pnpmPkgJson from '../pnpmPkgJson'
import {
  read as readYaml,
  write as writeYaml
} from './yamlfs'
import {PackageSpec, Resolution, TarballResolution, GitRepositoryResolution, DirectoryResolution} from '../resolve'

const shrinkwrapFilename = 'shrinkwrap.yaml'

export type Shrinkwrap = {
  [dependency: string]: Resolution
}

export function lookupResolution(shrinkwrap: Shrinkwrap, dependency: string): Resolution | null {
  let item = shrinkwrap[dependency]
  if (item) {
    item = item as any as TarballResolution // tslint:disable-line
    if (item.tarball != null) {
      return {...item, type: 'tarball'} as Resolution
    }
    item = item as any as GitRepositoryResolution // tslint:disable-line
    if (item.repo != null && item.commitId != null) {
      return {...item, type: 'git-repo'} as Resolution
    }
    item = item as any as DirectoryResolution // tslint:disable-line
    if (item.root != null) {
      return {...item, type: 'directory'} as Resolution
    }
  }
  return null
}

export function putResolution(shrinkwrap: Shrinkwrap, dependency: string, resolution: Resolution) {
  let {type, ...item} = resolution
  shrinkwrap[dependency] = item as Resolution
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
