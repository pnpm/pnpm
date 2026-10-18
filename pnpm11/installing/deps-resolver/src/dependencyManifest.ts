import { convertEnginesRuntimeToDependencies } from '@pnpm/pkg-manifest.utils'
import type { PackageManifest } from '@pnpm/types'
import { omit } from 'ramda'
import semver from 'semver'

import { getRangeOfLockedPackage } from './getDepsToResolve.js'
import { getPeerShadowedDependencies } from './hoistPeers.js'

export interface DependencyManifestContext {
  autoInstallPeers: boolean
  /** The aliases the package's parent depends on, which provide its peers. */
  parentPkgAliases: Record<string, unknown>
}

/**
 * The manifest of a resolved package as the resolver walks it, after the
 * read-package hook: the dependencies a peer supplies are dropped, and an
 * `engines.runtime` entry becomes a dependency. `pkg` may be modified.
 */
export function prepareDependencyManifest (pkg: PackageManifest, ctx: DependencyManifestContext): PackageManifest {
  pkg = omitPeersFromDependencies(pkg, ctx)
  if (pkg.engines?.runtime != null) {
    convertEnginesRuntimeToDependencies(pkg, 'engines', 'dependencies')
  }
  return pkg
}

function omitPeersFromDependencies (pkg: PackageManifest, ctx: DependencyManifestContext): PackageManifest {
  if (!pkg.dependencies || !pkg.peerDependencies) return pkg
  const shadowed = getPeerShadowedDependencies(pkg, {
    autoInstallPeers: ctx.autoInstallPeers,
    inScope: (alias) => Boolean(ctx.parentPkgAliases[alias]),
  })
  return { ...pkg, dependencies: omit(shadowed, pkg.dependencies) }
}

/**
 * Whether the version locked for a dependency still satisfies its wanted
 * specifier, as the resolver judges a locked reference. `undefined` when the
 * specifier is not a range the resolver checks (a tag, a git or tarball
 * specifier), which it resolves again.
 */
export function lockedVersionSatisfies (
  wantedDep: { alias: string, bareSpecifier: string },
  locked: { name: string, version: string, registryName?: string }
): boolean | undefined {
  const range = getRangeOfLockedPackage(wantedDep, locked)
  if (range == null) return false
  if (semver.validRange(range) == null) return undefined
  return semver.satisfies(locked.version, range, true)
}
