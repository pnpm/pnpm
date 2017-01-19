import {resolve} from 'path'
import * as path from 'path'
import getTarballName from './getTarballName'
import requireJson from '../fs/requireJson'
import {PackageSpec, ResolveOptions, Resolution} from '.'
import fs = require('mz/fs')

/**
 * Resolves a package hosted on the local filesystem
 */
export default async function resolveLocal (spec: PackageSpec, opts: ResolveOptions): Promise<Resolution> {
  const dependencyPath = resolve(opts.root, spec.spec)

  if (dependencyPath.slice(-4) === '.tgz' || dependencyPath.slice(-7) === '.tar.gz') {
    const name = getTarballName(dependencyPath)
    return {
      type: 'local-tarball',
      id: createLocalPkgId(name, dependencyPath),
      tarball: dependencyPath,
    }
  }

  if (opts.linkLocal) {
    const localPkg = await requireJson(resolve(dependencyPath, 'package.json'))
    return {
      type: 'link',
      id: createLocalPkgId(localPkg.name, dependencyPath),
      root: dependencyPath,
    }
  }
  return resolveFolder(dependencyPath)
}

async function resolveFolder (dependencyPath: string): Promise<Resolution> {
  const localPkg = await requireJson(resolve(dependencyPath, 'package.json'))
  return {
    type: 'directory',
    id: createLocalPkgId(localPkg.name, dependencyPath),
    root: dependencyPath,
  }
}

function createLocalPkgId (name: string, dependencyPath: string): string {
  return 'local/' + encodeURIComponent(dependencyPath)
}
