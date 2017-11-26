import fs = require('mz/fs')
import normalize = require('normalize-path')
import path = require('path')
import {
  DirectoryResolution,
  ResolveOptions,
  ResolveResult,
  TarballResolution,
  WantedDependency,
} from '..'
import {fromDir as readPkgFromDir} from '../../fs/readPkg'
import parsePref from './parsePref'

/**
 * Resolves a package hosted on the local filesystem
 */
export default async function resolveLocal (
  wantedDependency: WantedDependency,
  opts: ResolveOptions,
): Promise<ResolveResult | null> {
  const spec = parsePref(wantedDependency.pref, opts.prefix)
  if (!spec) return null

  const dependencyPath = normalize(path.relative(opts.prefix, spec.fetchSpec))
  const id = `file:${dependencyPath}`

  if (spec.type === 'file') {
    return {
      id,
      normalizedPref: spec.normalizedPref,
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
    normalizedPref: spec.normalizedPref,
    package: localPkg,
    resolution,
  }
}
