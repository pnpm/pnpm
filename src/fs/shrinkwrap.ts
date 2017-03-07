import path = require('path')
import {Resolution} from '../resolve'
import {PnpmError} from '../errorTypes'
import logger from 'pnpm-logger'
import loadYamlFile = require('load-yaml-file')
import writeYamlFile = require('write-yaml-file')
import R = require('ramda')
import rimraf = require('rimraf-then')
import isCI = require('is-ci')

const shrinkwrapLogger = logger('shrinkwrap')

const SHRINKWRAP_FILENAME = 'shrinkwrap.yaml'
const SHRINKWRAP_VERSION = 1

function getDefaultShrinkwrap () {
  return {
    version: SHRINKWRAP_VERSION,
    dependencies: {},
    packages: {},
  }
}

export type Shrinkwrap = {
  version: number,
  dependencies: ResolvedDependencies,
  packages: ResolvedPackages,
}

export type ResolvedPackages = {
  [pkgId: string]: DependencyShrinkwrap,
}

export type DependencyShrinkwrap = {
  resolution: Resolution,
  dependencies?: ResolvedDependencies,
}

/*** @example
 * {
 *   "foo": "registry.npmjs.org/foo/1.0.1"
 * }
 */
export type ResolvedDependencies = {
  [pkgName: string]: string,
}

export async function read (pkgPath: string, opts: {force: boolean}): Promise<Shrinkwrap> {
  const shrinkwrapPath = path.join(pkgPath, SHRINKWRAP_FILENAME)
  let shrinkwrap
  try {
    shrinkwrap = await loadYamlFile<Shrinkwrap>(shrinkwrapPath)
  } catch (err) {
    if ((<NodeJS.ErrnoException>err).code !== 'ENOENT') {
      throw err
    }
    return getDefaultShrinkwrap()
  }
  if (shrinkwrap && shrinkwrap.version === SHRINKWRAP_VERSION) {
    return shrinkwrap
  }
  if (opts.force || isCI) {
    shrinkwrapLogger.warn(`Ignoring not compatible shrinkwrap file at ${shrinkwrapPath}`)
    return getDefaultShrinkwrap()
  }
  throw new ShrinkwrapBreakingChangeError(shrinkwrapPath)
}

export function save (pkgPath: string, shrinkwrap: Shrinkwrap) {
  const shrinkwrapPath = path.join(pkgPath, SHRINKWRAP_FILENAME)

  // empty shrinkwrap is not saved
  if (Object.keys(shrinkwrap.dependencies).length === 0) {
    return rimraf(shrinkwrapPath)
  }

  const prunedShr = prune(shrinkwrap)

  return writeYamlFile(shrinkwrapPath, prunedShr, {
    sortKeys: true,
    lineWidth: 1000,
    noCompatMode: true,
  })
}

function prune (shr: Shrinkwrap): Shrinkwrap {
  return {
    version: SHRINKWRAP_VERSION,
    dependencies: shr.dependencies,
    packages: copyDependencyTree(shr),
  }
}

function copyDependencyTree (shr: Shrinkwrap): ResolvedPackages {
  const resolvedPackages: ResolvedPackages = {}
  let pkgIds: string[] = R.values(shr.dependencies)

  while (pkgIds.length) {
    let nextPkgIds: string[] = []
    for (let pkgId of pkgIds) {
      if (!shr.packages[pkgId]) {
        logger.warn(`Cannot find resolution of ${pkgId} in shrinkwrap file`)
        continue
      }
      resolvedPackages[pkgId] = shr.packages[pkgId]
      const newDependencies = R.values(shr.packages[pkgId].dependencies || {})
        .filter((newPkgId: string) => !resolvedPackages[newPkgId] && pkgIds.indexOf(newPkgId) === -1)
      nextPkgIds = R.union(nextPkgIds, newDependencies)
    }
    pkgIds = nextPkgIds
  }
  return resolvedPackages
}

class ShrinkwrapBreakingChangeError extends PnpmError {
  constructor (filename: string) {
    super('SHRINKWRAP_BREAKING_CHANGE', `Shrinkwrap file ${filename} not compatible with current pnpm`)
    this.filename = filename
  }
  filename: string
}
