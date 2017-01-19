import {resolve} from 'path'
import * as path from 'path'
import getTarballName from './getTarballName'
import requireJson from '../fs/requireJson'
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
      id: createLocalPkgId(name, dependencyPath),
      tarball: `file:${dependencyPath}`,
    }
    return {resolution}
  }

  if (opts.linkLocal) {
    const localPkg = await requireJson(resolve(dependencyPath, 'package.json'))
    const resolution: Resolution = {
      type: 'directory',
      id: createLocalPkgId(localPkg.name, dependencyPath),
      root: dependencyPath,
      link: true,
    }
    return {resolution}
  }
  return resolveFolder(dependencyPath)
}

async function resolveFolder (dependencyPath: string): Promise<ResolveResult> {
  const localPkg = await requireJson(resolve(dependencyPath, 'package.json'))
  const resolution: Resolution = {
    type: 'directory',
    id: createLocalPkgId(localPkg.name, dependencyPath),
    root: dependencyPath,
  }
  return {resolution}
}

function createLocalPkgId (name: string, dependencyPath: string): string {
  return 'local/' + encodeURIComponent(dependencyPath)
}
