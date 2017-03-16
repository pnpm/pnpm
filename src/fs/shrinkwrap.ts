import path = require('path')
import {Resolution, PackageSpec} from '../resolve'
import {PnpmError} from '../errorTypes'
import logger from 'pnpm-logger'
import loadYamlFile = require('load-yaml-file')
import writeYamlFile = require('write-yaml-file')
import R = require('ramda')
import rimraf = require('rimraf-then')
import isCI = require('is-ci')
import registryUrl = require('registry-url')
import getRegistryName from '../resolve/npm/getRegistryName'
import npa = require('npm-package-arg')

const shrinkwrapLogger = logger('shrinkwrap')

const SHRINKWRAP_FILENAME = 'shrinkwrap.yaml'
const PRIVATE_SHRINKWRAP_FILENAME = path.join('node_modules', '.shrinkwrap.yaml')
const SHRINKWRAP_VERSION = 2

function getDefaultShrinkwrap () {
  return {
    version: SHRINKWRAP_VERSION,
    dependencies: {},
    packages: {},
    registry: registryUrl(),
  }
}

export type Shrinkwrap = {
  version: number,
  dependencies: ResolvedDependencies,
  packages: ResolvedPackages,
  registry: string,
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

export async function readPrivate (pkgPath: string, opts: {force: boolean}): Promise<Shrinkwrap> {
  const shrinkwrapPath = path.join(pkgPath, PRIVATE_SHRINKWRAP_FILENAME)
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

export function prune (shr: Shrinkwrap): Shrinkwrap {
  return {
    version: SHRINKWRAP_VERSION,
    dependencies: shr.dependencies,
    registry: shr.registry,
    packages: copyDependencyTree(shr, shr.registry),
  }
}

function copyDependencyTree (shr: Shrinkwrap, registry: string): ResolvedPackages {
  const resolvedPackages: ResolvedPackages = {}

  let pkgIds: string[] = R.keys(shr.dependencies).map((rawPkgSpec: string) => {
    const spec: PackageSpec = npa(rawPkgSpec)
    return getPkgShortId(shr.dependencies[rawPkgSpec], spec.name)
  })

  while (pkgIds.length) {
    let nextPkgIds: string[] = []
    for (let pkgId of pkgIds) {
      if (!shr.packages[pkgId]) {
        logger.warn(`Cannot find resolution of ${pkgId} in shrinkwrap file`)
        continue
      }
      const depShr = shr.packages[pkgId]
      resolvedPackages[pkgId] = depShr
      const newDependencies = R.keys(depShr.dependencies)
        .map((pkgName: string) => getPkgShortId(<string>(depShr.dependencies && depShr.dependencies[pkgName]), pkgName))
        .filter((newPkgId: string) => !resolvedPackages[newPkgId] && pkgIds.indexOf(newPkgId) === -1)
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
  pkgVersion: string,
  resolution: Resolution,
  standardRegistry: string
) {
  if (resolution.type) return pkgId

  const registryName = getRegistryName(standardRegistry)
  if (pkgId.startsWith(`${registryName}/`)) {
    return pkgVersion
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
