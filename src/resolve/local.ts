import fs = require('mz/fs')
import normalize = require('normalize-path')
import path = require('path')
import {
  DirectoryResolution,
  PackageSpec,
  ResolveOptions,
  ResolveResult,
  TarballResolution,
} from '.'
import {fromDir as readPkgFromDir} from '../fs/readPkg'

/**
 * Resolves a package hosted on the local filesystem
 */
export default async function resolveLocal (spec: PackageSpec, opts: ResolveOptions): Promise<ResolveResult> {
  const dependencyPath = normalize(path.relative(opts.prefix, spec.fetchSpec))
  const id = `file:${dependencyPath}`

  if (spec.type === 'file') {
    return {
      id,
      resolution: { tarball: id },
    }
  }

  const localPkg = await readPkgFromDir(dependencyPath)
  const resolution: DirectoryResolution = {
    directory: dependencyPath,
    type: 'directory',
  }
  return {
    id,
    package: localPkg,
    resolution,
  }
}
