import path = require('path')
import {fromDir as readPkgFromDir} from '../fs/readPkg'
import {
  PackageSpec,
  ResolveOptions,
  TarballResolution,
  DirectoryResolution,
  ResolveResult,
} from '.'
import fs = require('mz/fs')
import normalize = require('normalize-path')

/**
 * Resolves a package hosted on the local filesystem
 */
export default async function resolveLocal (spec: PackageSpec, opts: ResolveOptions): Promise<ResolveResult> {
  const dependencyPath = normalize(path.relative(opts.prefix, spec.fetchSpec))
  const id = `file:${dependencyPath}`

  if (spec.type === 'file') {
    const resolution: TarballResolution = {
      tarball: id,
    }
    return {
      id,
      resolution,
    }
  }

  const localPkg = await readPkgFromDir(dependencyPath)
  const resolution: DirectoryResolution = {
    type: 'directory',
    directory: dependencyPath,
  }
  return {
    id,
    resolution,
  }
}
