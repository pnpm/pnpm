import {resolve} from 'path'
import * as path from 'path'
import readPkg from '../fs/readPkg'
import {
  PackageSpec,
  ResolveOptions,
  TarballResolution,
  DirectoryResolution,
  ResolveResult,
} from '.'
import fs = require('mz/fs')

/**
 * Resolves a package hosted on the local filesystem
 */
export default async function resolveLocal (spec: PackageSpec, opts: ResolveOptions): Promise<ResolveResult> {
  const dependencyPath = resolve(opts.root, spec.spec)

  if (dependencyPath.slice(-4) === '.tgz' || dependencyPath.slice(-7) === '.tar.gz') {
    const resolution: TarballResolution = {
      tarball: `file:${dependencyPath}`,
    }
    return {
      id: createLocalPkgId(dependencyPath),
      resolution,
    }
  }

  const localPkg = await readPkg(dependencyPath)
  const resolution: DirectoryResolution = {
    type: 'directory',
    root: dependencyPath,
  }
  return {
    id: createLocalPkgId(dependencyPath),
    resolution,
  }
}

function createLocalPkgId (dependencyPath: string): string {
  return 'local/' + encodeURIComponent(dependencyPath)
}
