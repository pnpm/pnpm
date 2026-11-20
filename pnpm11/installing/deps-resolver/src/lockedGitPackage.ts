import * as dp from '@pnpm/deps.path'
import type { LockfileResolution, PackageSnapshot, ResolvedDependencies } from '@pnpm/lockfile.types'
import { getLockfileImporterId } from '@pnpm/lockfile.utils'
import type { PackageResponse } from '@pnpm/store.controller-types'

import { getPeerShadowedDependencies } from './hoistPeers.js'
import type { DependencyRequest } from './requestDependencyPackage.js'

export function readLockedGitPackage (request: DependencyRequest): PackageResponse | undefined {
  const { currentPkg, ctx } = request
  const { dependencyLockfile: snapshot, name, pkgId, resolution, version } = currentPkg
  if (!snapshot || !name || !version || !pkgId || !resolution) return undefined
  if (!isGitResolution(snapshot.resolution) || !canReadLockedGitPackage(request, snapshot)) return undefined

  return {
    body: {
      id: pkgId,
      isLocal: false,
      manifest: {
        name,
        version,
        bundledDependencies: snapshot.bundledDependencies,
        cpu: snapshot.cpu,
        deprecated: snapshot.deprecated,
        engines: snapshot.engines,
        libc: snapshot.libc,
        os: snapshot.os,
        peerDependencies: snapshot.peerDependencies,
        peerDependenciesMeta: snapshot.peerDependenciesMeta,
        dependencies: specifiersFromReferences(snapshot.dependencies),
        optionalDependencies: specifiersFromReferences(snapshot.optionalDependencies),
      },
      resolution,
      publishedAt: ctx.wantedLockfile.time?.[pkgId],
      resolvedVia: 'git-repository',
      updated: false,
      // The lockfile already records the output of the read-package hook.
      hooked: true,
    },
  }
}

export function isGitResolution (resolution: LockfileResolution | undefined): boolean {
  return resolution != null && (('type' in resolution && resolution.type === 'git') || ('gitHosted' in resolution && resolution.gitHosted === true))
}

function canReadLockedGitPackage (
  { ctx, currentPkg, detached, options, wantedDependency }: DependencyRequest,
  snapshot: PackageSnapshot
): boolean {
  // Locked dependency references hold exact versions. Updating descendants
  // needs the ranges from the repository's manifest.
  const updatesDescendants = options.currentDepth < options.updateDepth
  const requiresFreshMetadata = options.update || updatesDescendants || options.updateChecksums || ctx.staleOverrideTargets?.size
  if (detached || !ctx.dryRun || !ctx.trustLockfile || !ctx.lockedPeersAreCurrent || requiresFreshMetadata) return false
  // A peer that is no longer supplied may have a regular dependency fallback.
  // The snapshot from the prior peer context cannot distinguish those cases.
  if (snapshot.peerDependencies != null) {
    const shadowed = getPeerShadowedDependencies(snapshot, {
      autoInstallPeers: ctx.autoInstallPeers,
      inScope: (alias) => Boolean(options.parentPkgAliases[alias]),
    })
    if (shadowed.length !== Object.keys(snapshot.peerDependencies).length) return false
  }
  const { alias, bareSpecifier } = wantedDependency
  if (options.currentDepth > 0) return bareSpecifier === currentPkg.pkgId
  const importer = getLockfileImporterId(ctx.lockfileDir, options.parentPkg.rootDir)
  return alias != null && ctx.wantedLockfile.importers[importer]?.specifiers[alias] === bareSpecifier
}

function specifiersFromReferences (refs?: ResolvedDependencies): Record<string, string> | undefined {
  if (refs == null) return undefined
  return Object.fromEntries(Object.entries(refs).map(([alias, ref]) => [alias, specifierFromReference(alias, ref)]))
}

function specifierFromReference (alias: string, ref: string): string {
  const depPath = dp.refToRelative(ref, alias)
  if (depPath == null) return ref
  const { name, nonSemverVersion, registryName, version } = dp.parse(depPath)
  if (nonSemverVersion != null) return nonSemverVersion
  if (name == null || version == null) return ref
  if (registryName != null) return `${registryName}:${name === alias ? version : `${name}@${version}`}`
  return name === alias ? version : `npm:${name}@${version}`
}
