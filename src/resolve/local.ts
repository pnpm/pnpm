import {resolve} from 'path'
import * as path from 'path'
import getTarballName from './getTarballName'
import readPkg from '../fs/readPkg'
import {PackageSpec, ResolveOptions, Resolution, ResolveResult} from '.'
import fs = require('mz/fs')

/**
 * Resolves a package hosted on the local filesystem
 */
export default async function resolveLocal (spec: PackageSpec, opts: ResolveOptions): Promise<ResolveResult> {
  const dependencyPath = resolve(opts.root, spec.spec)

  if (dependencyPath.slice(-4) === '.tgz' || dependencyPath.slice(-7) === '.tar.gz') {
    const name = getTarballName(dependencyPath)
    const resolution: Resolution = {
      type: 'tarball',
      tarball: `file:${dependencyPath}`,
    }
    return {
      id: createLocalPkgId(name, dependencyPath),
      resolution,
    }
  }

  const localPkg = await readPkg(dependencyPath)
  const resolution: Resolution = {
    type: 'directory',
    root: dependencyPath,
  }
  return {
    id: createLocalPkgId(localPkg.name, dependencyPath),
    resolution,
  }
}

function createLocalPkgId (name: string, dependencyPath: string): string {
  return 'local/' + encodeURIComponent(dependencyPath)
}
