import logger from '@pnpm/logger'
import {
  Lockfile,
  LockfileResolution,
  PackageSnapshot,
  pruneSharedLockfile,
  ResolvedDependencies,
} from '@pnpm/prune-lockfile'
import { Resolution } from '@pnpm/resolver-base'
import { Registries } from '@pnpm/types'
import * as dp from 'dependency-path'
import getNpmTarballUrl from 'get-npm-tarball-url'
import * as R from 'ramda'
import depPathToRef from './depPathToRef'
import { DependenciesGraph } from '.'
import { ResolvedPackage } from './resolveDependencies'

export default function (
  depGraph: DependenciesGraph,
  lockfile: Lockfile,
  prefix: string,
  registries: Registries
): {
    newLockfile: Lockfile
    pendingRequiresBuilds: string[]
  } {
  lockfile.packages = lockfile.packages ?? {}
  const pendingRequiresBuilds = [] as string[]
  for (const depPath of Object.keys(depGraph)) {
    const depNode = depGraph[depPath]
    const [updatedOptionalDeps, updatedDeps] = R.partition(
      (child) => depNode.optionalDependencies.has(child.alias),
      Object.keys(depNode.children).map((alias) => ({ alias, depPath: depNode.children[alias] }))
    )
    lockfile.packages[depPath] = toLockfileDependency(pendingRequiresBuilds, depNode, {
      depGraph,
      depPath,
      prevSnapshot: lockfile.packages[depPath],
      registries,
      registry: dp.getRegistryByPackageName(registries, depNode.name),
      updatedDeps,
      updatedOptionalDeps,
    })
  }
  const warn = (message: string) => logger.warn({ message, prefix })
  return {
    newLockfile: pruneSharedLockfile(lockfile, { warn }),
    pendingRequiresBuilds,
  }
}

function toLockfileDependency (
  pendingRequiresBuilds: string[],
  pkg: ResolvedPackage,
  opts: {
    depPath: string
    registry: string
    registries: Registries
    updatedDeps: Array<{alias: string, depPath: string}>
    updatedOptionalDeps: Array<{alias: string, depPath: string}>
    depGraph: DependenciesGraph
    prevSnapshot?: PackageSnapshot
  }
): PackageSnapshot {
  const lockfileResolution = toLockfileResolution(
    { name: pkg.name, version: pkg.version },
    opts.depPath,
    pkg.resolution,
    opts.registry
  )
  const newResolvedDeps = updateResolvedDeps(
    opts.prevSnapshot?.dependencies ?? {},
    opts.updatedDeps,
    opts.registries,
    opts.depGraph
  )
  const newResolvedOptionalDeps = updateResolvedDeps(
    opts.prevSnapshot?.optionalDependencies ?? {},
    opts.updatedOptionalDeps,
    opts.registries,
    opts.depGraph
  )
  const result = {
    resolution: lockfileResolution,
  }
  /* eslint-disable @typescript-eslint/dot-notation */
  if (dp.isAbsolute(opts.depPath)) {
    result['name'] = pkg.name

    // There is no guarantee that a non-npmjs.org-hosted package
    // is going to have a version field
    if (pkg.version) {
      result['version'] = pkg.version
    }
  }
  if (!R.isEmpty(newResolvedDeps)) {
    result['dependencies'] = newResolvedDeps
  }
  if (!R.isEmpty(newResolvedOptionalDeps)) {
    result['optionalDependencies'] = newResolvedOptionalDeps
  }
  if (pkg.dev && !pkg.prod) {
    result['dev'] = true
  } else if (pkg.prod && !pkg.dev) {
    result['dev'] = false
  }
  if (pkg.optional) {
    result['optional'] = true
  }
  if (opts.depPath[0] !== '/' && !pkg.id.endsWith(opts.depPath)) {
    result['id'] = pkg.id
  }
  if (!R.isEmpty(pkg.peerDependencies ?? {})) {
    result['peerDependencies'] = pkg.peerDependencies
  }
  if (pkg.peerDependenciesMeta) {
    const normalizedPeerDependenciesMeta = {}
    for (const peer of Object.keys(pkg.peerDependenciesMeta)) {
      if (pkg.peerDependenciesMeta[peer].optional) {
        normalizedPeerDependenciesMeta[peer] = { optional: true }
      }
    }
    if (Object.keys(normalizedPeerDependenciesMeta).length) {
      result['peerDependenciesMeta'] = normalizedPeerDependenciesMeta
    }
  }
  if (pkg.additionalInfo.engines) {
    for (const engine of R.keys(pkg.additionalInfo.engines)) {
      if (pkg.additionalInfo.engines[engine] === '*') continue
      result['engines'] = result['engines'] || {}
      result['engines'][engine] = pkg.additionalInfo.engines[engine]
    }
  }
  if (pkg.additionalInfo.cpu) {
    result['cpu'] = pkg.additionalInfo.cpu
  }
  if (pkg.additionalInfo.os) {
    result['os'] = pkg.additionalInfo.os
  }
  if (Array.isArray(pkg.additionalInfo.bundledDependencies) || Array.isArray(pkg.additionalInfo.bundleDependencies)) {
    result['bundledDependencies'] = pkg.additionalInfo.bundledDependencies ?? pkg.additionalInfo.bundleDependencies
  }
  if (pkg.additionalInfo.deprecated) {
    result['deprecated'] = pkg.additionalInfo.deprecated
  }
  if (pkg.hasBin) {
    result['hasBin'] = true
  }
  if (opts.prevSnapshot) {
    if (opts.prevSnapshot.requiresBuild) {
      result['requiresBuild'] = opts.prevSnapshot.requiresBuild
    }
    if (opts.prevSnapshot.prepare) {
      result['prepare'] = opts.prevSnapshot.prepare
    }
  } else if (pkg.prepare) {
    result['prepare'] = true
    result['requiresBuild'] = true
  } else if (pkg.requiresBuild !== undefined) {
    if (pkg.requiresBuild) {
      result['requiresBuild'] = true
    }
  } else {
    pendingRequiresBuilds.push(opts.depPath)
  }
  pkg.requiresBuild = result['requiresBuild']
  /* eslint-enable @typescript-eslint/dot-notation */
  return result
}

// previous resolutions should not be removed from lockfile
// as installation might not reanalize the whole dependency graph
// the `depth` property defines how deep should dependencies be checked
function updateResolvedDeps (
  prevResolvedDeps: ResolvedDependencies,
  updatedDeps: Array<{alias: string, depPath: string}>,
  registries: Registries,
  depGraph: DependenciesGraph
) {
  const newResolvedDeps = R.fromPairs<string>(
    updatedDeps
      .map(({ alias, depPath }): R.KeyValuePair<string, string> => {
        if (depPath.startsWith('link:')) {
          return [alias, depPath]
        }
        const depNode = depGraph[depPath]
        return [
          alias,
          depPathToRef(depNode.depPath, {
            alias,
            realName: depNode.name,
            registries,
            resolution: depNode.resolution,
          }),
        ]
      })
  )
  return R.merge(
    prevResolvedDeps,
    newResolvedDeps
  )
}

function toLockfileResolution (
  pkg: {
    name: string
    version: string
  },
  depPath: string,
  resolution: Resolution,
  registry: string
): LockfileResolution {
  /* eslint-disable @typescript-eslint/dot-notation */
  if (dp.isAbsolute(depPath) || resolution.type !== undefined || !resolution['integrity']) {
    return resolution as LockfileResolution
  }
  const base = registry !== resolution['registry'] ? { registry: resolution['registry'] } : {}
  // Sometimes packages are hosted under non-standard tarball URLs.
  // For instance, when they are hosted on npm Enterprise. See https://github.com/pnpm/pnpm/issues/867
  // Or in othere weird cases, like https://github.com/pnpm/pnpm/issues/1072
  const expectedTarball = getNpmTarballUrl(pkg.name, pkg.version, { registry })
  const actualTarball = resolution['tarball'].replace('%2f', '/')
  if (removeProtocol(expectedTarball) !== removeProtocol(actualTarball)) {
    return {
      ...base,
      integrity: resolution['integrity'],
      tarball: relativeTarball(resolution['tarball'], registry),
    }
  }
  return {
    ...base,
    integrity: resolution['integrity'],
  }
  /* eslint-enable @typescript-eslint/dot-notation */
}

function removeProtocol (url: string) {
  return url.split('://')[1]
}

function relativeTarball (tarball: string, registry: string) {
  // It is important to save the tarball URL as "relative-path" (without the leading '/').
  // Sometimes registries are located in a subdirectory of a website.
  // For instance, https://mycompany.jfrog.io/mycompany/api/npm/npm-local/
  // So the tarball location should be relative to the directory,
  // it is not an absolute-path reference.
  // So we add @mycompany/mypackage/-/@mycompany/mypackage-2.0.0.tgz
  // not /@mycompany/mypackage/-/@mycompany/mypackage-2.0.0.tgz
  // Related issue: https://github.com/pnpm/pnpm/issues/1827
  if (tarball.substr(0, registry.length) === registry) {
    return tarball.substr(registry.length)
  }
  return tarball
}
