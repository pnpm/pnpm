import path = require('path')
import {Resolution, PackageSpec} from '../resolve'
import {PnpmError} from '../errorTypes'
import logger from 'pnpm-logger'
import loadYamlFile = require('load-yaml-file')
import writeYamlFile = require('write-yaml-file')
import R = require('ramda')
import rimraf = require('rimraf-then')
import isCI = require('is-ci')
import getRegistryName from '../resolve/npm/getRegistryName'
import npa = require('npm-package-arg')
import pnpmPkgJson from '../pnpmPkgJson'
import {Package} from '../types'

const shrinkwrapLogger = logger('shrinkwrap')

export const SHRINKWRAP_FILENAME = 'shrinkwrap.yaml'
export const PRIVATE_SHRINKWRAP_FILENAME = path.join('node_modules', '.shrinkwrap.yaml')
const SHRINKWRAP_VERSION = 2
const CREATED_WITH = `${pnpmPkgJson.name}@${pnpmPkgJson.version}`

function getDefaultShrinkwrap (registry: string) {
  return {
    version: SHRINKWRAP_VERSION,
    createdWith: CREATED_WITH,
    specifiers: {},
    dependencies: {},
    packages: {},
    registry,
  }
}

export type Shrinkwrap = {
  version: number,
  createdWith: string,
  specifiers: ResolvedDependencies,
  dependencies: ResolvedDependencies,
  packages: ResolvedPackages,
  registry: string,
}

export type ResolvedPackages = {
  [pkgId: string]: DependencyShrinkwrap,
}

export type DependencyShrinkwrap = {
  id?: string,
  dev?: true,
  optional?: true,
  resolution: string | Resolution,
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

export async function readPrivate (
  pkgPath: string,
  opts: {
    force: boolean,
    registry: string,
  }
): Promise<Shrinkwrap> {
  const shrinkwrapPath = path.join(pkgPath, PRIVATE_SHRINKWRAP_FILENAME)
  let shrinkwrap
  try {
    shrinkwrap = await loadYamlFile<Shrinkwrap>(shrinkwrapPath)
  } catch (err) {
    if ((<NodeJS.ErrnoException>err).code !== 'ENOENT') {
      throw err
    }
    return getDefaultShrinkwrap(opts.registry)
  }
  if (shrinkwrap && shrinkwrap.version === SHRINKWRAP_VERSION) {
    return shrinkwrap
  }
  if (opts.force || isCI) {
    shrinkwrapLogger.warn(`Ignoring not compatible shrinkwrap file at ${shrinkwrapPath}`)
    return getDefaultShrinkwrap(opts.registry)
  }
  throw new ShrinkwrapBreakingChangeError(shrinkwrapPath)
}

export async function read (
  pkgPath: string,
  opts: {
    force: boolean,
    registry: string,
}): Promise<Shrinkwrap> {
  const shrinkwrapPath = path.join(pkgPath, SHRINKWRAP_FILENAME)
  let shrinkwrap
  try {
    shrinkwrap = await loadYamlFile<Shrinkwrap>(shrinkwrapPath)
  } catch (err) {
    if ((<NodeJS.ErrnoException>err).code !== 'ENOENT') {
      throw err
    }
    return getDefaultShrinkwrap(opts.registry)
  }
  if (shrinkwrap && shrinkwrap.version === SHRINKWRAP_VERSION) {
    return shrinkwrap
  }
  if (opts.force || isCI) {
    shrinkwrapLogger.warn(`Ignoring not compatible shrinkwrap file at ${shrinkwrapPath}`)
    return getDefaultShrinkwrap(opts.registry)
  }
  throw new ShrinkwrapBreakingChangeError(shrinkwrapPath)
}

export function save (pkgPath: string, shrinkwrap: Shrinkwrap) {
  const shrinkwrapPath = path.join(pkgPath, SHRINKWRAP_FILENAME)
  const privateShrinkwrapPath = path.join(pkgPath, PRIVATE_SHRINKWRAP_FILENAME)

  // empty shrinkwrap is not saved
  if (Object.keys(shrinkwrap.dependencies).length === 0) {
    return Promise.all([
      rimraf(shrinkwrapPath),
      rimraf(privateShrinkwrapPath),
    ])
  }

  const formatOpts = {
    sortKeys: true,
    lineWidth: 1000,
    noCompatMode: true,
  }

  return Promise.all([
    writeYamlFile(shrinkwrapPath, shrinkwrap, formatOpts),
    writeYamlFile(privateShrinkwrapPath, shrinkwrap, formatOpts),
  ])
}

export function prune (shr: Shrinkwrap, pkg: Package): Shrinkwrap {
  const packages: ResolvedPackages = {}
  const optionalDependencies = R.keys(pkg.optionalDependencies)
  const dependencies = R.difference(R.keys(pkg.dependencies), optionalDependencies)
  const devDependencies = R.difference(R.difference(R.keys(pkg.devDependencies), optionalDependencies), dependencies)
  copyDependencyTree(packages, shr, {
    registry: shr.registry,
    dependencies: devDependencies,
    dev: true,
  })
  copyDependencyTree(packages, shr, {
    registry: shr.registry,
    dependencies: optionalDependencies,
    optional: true,
  })
  copyDependencyTree(packages, shr, {
    registry: shr.registry,
    dependencies,
  })
  return {
    version: SHRINKWRAP_VERSION,
    createdWith: shr.createdWith || CREATED_WITH,
    specifiers: shr.specifiers,
    registry: shr.registry,
    dependencies: shr.dependencies,
    packages,
  }
}

function copyDependencyTree (
  resolvedPackages: ResolvedPackages,
  shr: Shrinkwrap,
  opts: {
    registry: string,
    dependencies: string[],
    dev?: boolean,
    optional?: boolean,
  }
): ResolvedPackages {
  let pkgIds: string[] = opts.dependencies
    .map((pkgName: string) => getPkgShortId(shr.dependencies[pkgName], pkgName))
  const checked = new Set<string>()

  while (pkgIds.length) {
    let nextPkgIds: string[] = []
    for (let pkgId of pkgIds) {
      if (checked.has(pkgId)) continue
      checked.add(pkgId)
      if (!shr.packages[pkgId]) {
        logger.warn(`Cannot find resolution of ${pkgId} in shrinkwrap file`)
        continue
      }
      const depShr = shr.packages[pkgId]
      resolvedPackages[pkgId] = depShr
      if (opts.optional) {
        depShr.optional = true
      } else {
        delete depShr.optional
      }
      if (opts.dev) {
        depShr.dev = true
      } else {
        delete depShr.dev
      }
      const newDependencies = R.keys(depShr.dependencies)
        .map((pkgName: string) => getPkgShortId(<string>(depShr.dependencies && depShr.dependencies[pkgName]), pkgName))
        .filter((newPkgId: string) => !checked.has(newPkgId))
      nextPkgIds = R.union(nextPkgIds, newDependencies)
    }
    pkgIds = nextPkgIds
  }
  return resolvedPackages
}

export function shortIdToFullId (
  shortId: string,
  registry: string
) {
  if (shortId[0] === '/') {
    const registryName = getRegistryName(registry)
    return `${registryName}${shortId}`
  }
  return shortId
}

export function getPkgShortId (
  reference: string,
  pkgName: string
) {
  if (reference.indexOf('/') === -1) {
    return `/${pkgName}/${reference}`
  }
  return reference
}

export function getPkgId (
  reference: string,
  pkgName: string,
  registry: string
) {
  if (reference.indexOf('/') === -1) {
    const registryName = getRegistryName(registry)
    return `${registryName}/${pkgName}/${reference}`
  }
  return reference
}

export function pkgIdToRef (
  pkgId: string,
  pkgName: string,
  resolution: Resolution,
  standardRegistry: string
) {
  if (resolution.type) return pkgId

  const registryName = getRegistryName(standardRegistry)
  if (pkgId.startsWith(`${registryName}/`)) {
    const ref = pkgId.replace(`${registryName}/${pkgName}/`, '')
    if (ref.indexOf('/') === -1) return ref
    return pkgId.replace(`${registryName}/`, '/')
  }
  return pkgId
}

export function pkgShortId (
  pkgId: string,
  standardRegistry: string
) {
  const registryName = getRegistryName(standardRegistry)

  if (pkgId.startsWith(`${registryName}/`)) {
    return pkgId.substr(pkgId.indexOf('/'))
  }
  return pkgId
}

class ShrinkwrapBreakingChangeError extends PnpmError {
  constructor (filename: string) {
    super('SHRINKWRAP_BREAKING_CHANGE', `Shrinkwrap file ${filename} not compatible with current pnpm`)
    this.filename = filename
  }
  filename: string
}
