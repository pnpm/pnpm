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
  let lockfileResolution = toLockfileResolution(
    { name: pkg.name, version: pkg.version },
    pkg.resolution,
    {
      registry: opts.registry,
      serverType: opts.serverType,
      lockfileIncludeTarballUrl: opts.lockfileIncludeTarballUrl,
    }
  )

  if (
    'tarball' in lockfileResolution &&
    lockfileResolution.integrity == null &&
    lockfileResolution.type === undefined
  ) {
    const prevResolution = opts.prevSnapshot?.resolution
    if (
      prevResolution != null &&
      'tarball' in prevResolution &&
      prevResolution.type === undefined &&
      prevResolution.tarball === lockfileResolution.tarball &&
      prevResolution.integrity != null
    ) {
      lockfileResolution = { ...lockfileResolution, integrity: prevResolution.integrity }
    }
  }

  const newResolvedDeps = updateResolvedDeps(
    opts.updatedDeps,
    opts.depGraph
  )
  const newResolvedOptionalDeps = updateResolvedDeps(
    opts.updatedOptionalDeps,
    opts.depGraph
  )
  const result = {
    resolution: lockfileResolution,
  } as PackageSnapshot
  // A registry-qualified dep path (`<name>@<registryName>:<version>`) already
  // carries a parseable semver, so the explicit version field written for
  // other `:`-containing dep paths would be redundant.
  if (opts.depPath.includes(':') && opts.registryName == null) {
    // There is no guarantee that a non-npmjs.org-hosted package is going to have a version field.
    // Also, for local directory dependencies, the version is not needed.
    if (
      pkg.version &&
      (
        !('type' in lockfileResolution) ||
        lockfileResolution.type !== 'directory'
      )
    ) {
      result['version'] = pkg.version
    }
  }
  if (Object.keys(newResolvedDeps).length > 0) {
    result['dependencies'] = newResolvedDeps
  }
  if (Object.keys(newResolvedOptionalDeps).length > 0) {
    result['optionalDependencies'] = newResolvedOptionalDeps
  }
  if (pkg.optional) {
    result['optional'] = true
  }
  if (pkg.transitivePeerDependencies.size) {
    result['transitivePeerDependencies'] = Array.from(pkg.transitivePeerDependencies).sort()
  }
  if (Object.keys(pkg.peerDependencies ?? {}).length > 0) {
    const peerPkgs: Record<string, string> = {}
    const normalizedPeerDependenciesMeta: Record<string, { optional: true }> = {}
    for (const [peer, { version, optional }] of Object.entries(pkg.peerDependencies)) {
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
  if (pkg.additionalInfo.engines != null) {
    for (const [engine, version] of Object.entries(pkg.additionalInfo.engines)) {
      if (version === '*') continue
      result.engines = result.engines ?? {} as any // eslint-disable-line @typescript-eslint/no-explicit-any
      result.engines![engine] = version
    }
  }
  if (pkg.additionalInfo.cpu != null) {
    result['cpu'] = pkg.additionalInfo.cpu
  }
  if (pkg.additionalInfo.os != null) {
    result['os'] = pkg.additionalInfo.os
  }
  if (pkg.additionalInfo.libc != null) {
    result['libc'] = pkg.additionalInfo.libc
  }
  if (
    (Array.isArray(pkg.additionalInfo.bundledDependencies) && pkg.additionalInfo.bundledDependencies.length > 0) ||
    pkg.additionalInfo.bundledDependencies === true
  ) {
    result['bundledDependencies'] = pkg.additionalInfo.bundledDependencies
  } else if (
    (Array.isArray(pkg.additionalInfo.bundleDependencies) && pkg.additionalInfo.bundleDependencies.length > 0) ||
    pkg.additionalInfo.bundleDependencies === true
  ) {
    result['bundledDependencies'] = pkg.additionalInfo.bundleDependencies
  }
  if (pkg.additionalInfo.deprecated) {
    result['deprecated'] = pkg.additionalInfo.deprecated
  } else if (
    // `deprecated` is the only registry-mutable field of a published
    // version; an unchanged resolution must not lose a recorded
    // deprecation to a registry serving it inconsistently
    // (pnpm/pnpm#13846).
    opts.prevSnapshot?.deprecated != null &&
    equals(opts.prevSnapshot.resolution, lockfileResolution)
  ) {
    result['deprecated'] = opts.prevSnapshot.deprecated
  }
  if (pkg.hasBin) {
    result['hasBin'] = true
  }
  if (pkg.patch) {
    result['patched'] = true
  }
  return result
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
