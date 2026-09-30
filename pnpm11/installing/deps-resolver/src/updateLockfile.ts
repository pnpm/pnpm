import { getRegistryServerType, normalizeRegistriesByPrefix } from '@pnpm/config.normalize-registries'
import * as dp from '@pnpm/deps.path'
import {
  type LockfileObject,
  type PackageSnapshot,
  pruneSharedLockfile,
} from '@pnpm/lockfile.pruner'
import { toLockfileResolution } from '@pnpm/lockfile.utils'
import { logger } from '@pnpm/logger'
import type { DepPath, RegistriesByScope, RegistryContext, RegistryServerType } from '@pnpm/types'
import type { KeyValuePair } from 'ramda'
import { equals, partition } from 'ramda'

import { depPathToRef } from './depPathToRef.js'
import type { DependenciesGraph } from './index.js'
import type { ResolvedPackage } from './resolveDependencies.js'

export function updateLockfile (
  { dependenciesGraph, lockfile, prefix, registriesByScope, registriesByPrefix, registryOptionsByUrl, lockfileIncludeTarballUrl }: RegistryContext & {
    dependenciesGraph: DependenciesGraph
    lockfile: LockfileObject
    prefix: string
    lockfileIncludeTarballUrl?: boolean
  }
): LockfileObject {
  lockfile.packages = lockfile.packages ?? {}
  const mergedRegistriesByPrefix = normalizeRegistriesByPrefix(registriesByPrefix)
  for (const [depPath, depNode] of Object.entries(dependenciesGraph)) {
    const [updatedOptionalDeps, updatedDeps] = partition(
      (child) => depNode.optionalDependencies.has(child.alias) || depNode.peerDependencies[child.alias]?.optional === true,
      Object.entries<DepPath>(depNode.children).map(([alias, depPath]) => ({ alias, depPath }))
    )
    // The registry decides whether the tarball URL is canonical (and can be
    // dropped from the lockfile entry): a registry-qualified dep path is
    // checked against its named registry, everything else against the
    // scope-routed one.
    const registryName = dp.parse(depPath).registryName
    const registry = (registryName != null ? mergedRegistriesByPrefix[registryName] : undefined) ??
      dp.getRegistryByPackageName(registriesByScope, depNode.name)
    lockfile.packages[depPath as DepPath] = toLockfileDependency(depNode, {
      depGraph: dependenciesGraph,
      depPath,
      prevSnapshot: lockfile.packages[depPath as DepPath],
      registriesByScope,
      registry,
      serverType: getRegistryServerType({ registryOptionsByUrl }, registry),
      registryName,
      updatedDeps,
      updatedOptionalDeps,
      lockfileIncludeTarballUrl,
    })
  }
  const warn = (message: string) => {
    logger.warn({ message, prefix })
  }
  return pruneSharedLockfile(lockfile, { warn, dependenciesGraph })
}

type LockfileResolution = PackageSnapshot['resolution']

type AdditionalInfo = ResolvedPackage['additionalInfo']

function toLockfileDependency (
  pkg: ResolvedPackage & { transitivePeerDependencies: Set<string> },
  opts: {
    depPath: string
    registry: string
    serverType?: RegistryServerType
    registryName?: string
    registriesByScope: RegistriesByScope
    updatedDeps: Array<{ alias: string, depPath: DepPath }>
    updatedOptionalDeps: Array<{ alias: string, depPath: DepPath }>
    depGraph: DependenciesGraph
    prevSnapshot?: PackageSnapshot
    lockfileIncludeTarballUrl?: boolean
  }
): PackageSnapshot {
  const lockfileResolution = keepRecordedTarballIntegrity(
    toLockfileResolution(
      { name: pkg.name, version: pkg.version },
      pkg.resolution,
      {
        registry: opts.registry,
        serverType: opts.serverType,
        lockfileIncludeTarballUrl: opts.lockfileIncludeTarballUrl,
      }
    ),
    opts.prevSnapshot
  )

  const result = {
    resolution: lockfileResolution,
  } as PackageSnapshot
  if (shouldRecordVersion(pkg.version, { depPath: opts.depPath, registryName: opts.registryName, lockfileResolution })) {
    result['version'] = pkg.version
  }
  addChildDependencies(result, opts)
  if (pkg.optional) {
    result['optional'] = true
  }
  if (pkg.transitivePeerDependencies.size) {
    result['transitivePeerDependencies'] = Array.from(pkg.transitivePeerDependencies).sort()
  }
  addPeerDependencies(result, pkg.peerDependencies ?? {})
  addPlatformRequirements(result, pkg.additionalInfo)
  const deprecated = getDeprecationToRecord(opts.prevSnapshot, lockfileResolution, pkg.additionalInfo.deprecated)
  if (deprecated != null) {
    result['deprecated'] = deprecated
  }
  if (pkg.hasBin) {
    result['hasBin'] = true
  }
  if (pkg.patch) {
    result['patched'] = true
  }
  return result
}

function addChildDependencies (
  result: PackageSnapshot,
  opts: {
    updatedDeps: Array<{ alias: string, depPath: DepPath }>
    updatedOptionalDeps: Array<{ alias: string, depPath: DepPath }>
    depGraph: DependenciesGraph
  }
): void {
  const newResolvedDeps = updateResolvedDeps(
    opts.updatedDeps,
    opts.depGraph
  )
  const newResolvedOptionalDeps = updateResolvedDeps(
    opts.updatedOptionalDeps,
    opts.depGraph
  )
  if (Object.keys(newResolvedDeps).length > 0) {
    result['dependencies'] = newResolvedDeps
  }
  if (Object.keys(newResolvedOptionalDeps).length > 0) {
    result['optionalDependencies'] = newResolvedOptionalDeps
  }
}

function keepRecordedTarballIntegrity (
  lockfileResolution: LockfileResolution,
  prevSnapshot: PackageSnapshot | undefined
): LockfileResolution {
  if (
    !('tarball' in lockfileResolution) ||
    lockfileResolution.integrity != null ||
    lockfileResolution.type !== undefined
  ) return lockfileResolution
  const prevResolution = prevSnapshot?.resolution
  if (
    prevResolution != null &&
    'tarball' in prevResolution &&
    prevResolution.type === undefined &&
    prevResolution.tarball === lockfileResolution.tarball &&
    prevResolution.integrity != null
  ) {
    return { ...lockfileResolution, integrity: prevResolution.integrity }
  }
  return lockfileResolution
}

// A registry-qualified dep path (`<name>@<registryName>:<version>`) already
// carries a parseable semver, so the explicit version field written for
// other `:`-containing dep paths would be redundant.
function shouldRecordVersion (
  version: string,
  opts: {
    depPath: string
    registryName?: string
    lockfileResolution: LockfileResolution
  }
): boolean {
  if (!opts.depPath.includes(':') || opts.registryName != null) return false
  // There is no guarantee that a non-npmjs.org-hosted package is going to have a version field.
  // Also, for local directory dependencies, the version is not needed.
  return Boolean(version) &&
    (
      !('type' in opts.lockfileResolution) ||
      opts.lockfileResolution.type !== 'directory'
    )
}

function addPeerDependencies (
  result: PackageSnapshot,
  peerDependencies: NonNullable<ResolvedPackage['peerDependencies']>
): void {
  if (Object.keys(peerDependencies).length === 0) return
  const peerPkgs: Record<string, string> = {}
  const normalizedPeerDependenciesMeta: Record<string, { optional: true }> = {}
  for (const [peer, { version, optional }] of Object.entries(peerDependencies)) {
    peerPkgs[peer] = version
    if (optional) {
      normalizedPeerDependenciesMeta[peer] = { optional: true }
    }
  }
  result['peerDependencies'] = peerPkgs
  if (Object.keys(normalizedPeerDependenciesMeta).length > 0) {
    result['peerDependenciesMeta'] = normalizedPeerDependenciesMeta
  }
}

function addPlatformRequirements (result: PackageSnapshot, additionalInfo: AdditionalInfo): void {
  addEngines(result, additionalInfo.engines)
  if (additionalInfo.cpu != null) {
    result['cpu'] = additionalInfo.cpu
  }
  if (additionalInfo.os != null) {
    result['os'] = additionalInfo.os
  }
  if (additionalInfo.libc != null) {
    result['libc'] = additionalInfo.libc
  }
  if (declaresBundledDependencies(additionalInfo.bundledDependencies)) {
    result['bundledDependencies'] = additionalInfo.bundledDependencies
  } else if (declaresBundledDependencies(additionalInfo.bundleDependencies)) {
    result['bundledDependencies'] = additionalInfo.bundleDependencies
  }
}

function addEngines (result: PackageSnapshot, engines: AdditionalInfo['engines']): void {
  // The legacy array form, such as `["node >= 0.8"]`, is not checked for
  // installability, so it is not recorded either.
  if (engines == null || Array.isArray(engines)) return
  for (const [engine, version] of Object.entries(engines)) {
    if (version === '*') continue
    result.engines = result.engines ?? {} as any // eslint-disable-line @typescript-eslint/no-explicit-any -- the engines type requires a node field that a package may not declare
    result.engines![engine] = version
  }
}

function declaresBundledDependencies (bundledDependencies: AdditionalInfo['bundledDependencies']): boolean {
  return (Array.isArray(bundledDependencies) && bundledDependencies.length > 0) ||
    bundledDependencies === true
}

// `deprecated` is the only registry-mutable field of a published
// version. An unchanged resolution keeps its recorded deprecation, so
// neither a registry serving it inconsistently (pnpm/pnpm#13846) nor a
// stale metadata cache (pnpm/pnpm#5772) can rewrite it.
function getDeprecationToRecord (
  prevSnapshot: PackageSnapshot | undefined,
  lockfileResolution: LockfileResolution,
  deprecated: AdditionalInfo['deprecated']
): string | undefined {
  if (prevSnapshot?.deprecated != null && equals(prevSnapshot.resolution, lockfileResolution)) {
    return prevSnapshot.deprecated
  }
  return deprecated || undefined
}

function updateResolvedDeps (
  updatedDeps: Array<{ alias: string, depPath: DepPath }>,
  depGraph: DependenciesGraph
): Record<string, string> {
  return Object.fromEntries(
    updatedDeps
      .map(({ alias, depPath }): KeyValuePair<string, string> => {
        if (depPath.startsWith('link:')) {
          return [alias, depPath]
        }
        const depNode = depGraph[depPath]
        return [
          alias,
          depPathToRef(depPath, {
            alias,
            realName: depNode.name,
          }),
        ]
      })
  )
}
