import { getRegistryServerType } from '@pnpm/config.normalize-registries'
import type { VersionOverride } from '@pnpm/config.parse-overrides'
import * as dp from '@pnpm/deps.path'
import type {
  LockfileObject,
  PackageSnapshot,
  ResolvedDependencies,
} from '@pnpm/lockfile.types'
import { nameVerFromPkgSnapshot } from '@pnpm/lockfile.utils'
import { toLockfileResolution } from '@pnpm/lockfile.utils'
import type { RequestPackageFunction } from '@pnpm/store.controller-types'
import type {
  DepPath,
  PackageManifest,
  PackageVersionPolicy,
  ReadPackageHook,
  RegistriesByScope,
  RegistryOptions,
  RegistryServerType,
  TrustPolicy,
} from '@pnpm/types'
import { clone, equals } from 'ramda'
import semver from 'semver'

export interface FastOverride {
  name: string
  newVersion?: string
  oldVersion?: string
  parent?: {
    name: string
    bareSpecifier?: string
  }
}

interface RewriteContext {
  changedNames: Set<string>
  peerNames: Set<string>
  removals: FastOverride[]
  replacements: Map<DepPath, DepPath>
  /** The replacing overrides, needed to scope a `parent>child` selector. */
  moves: FastOverride[]
}

interface ResolverPolicyOptions {
  publishedBy?: Date
  publishedByExclude?: PackageVersionPolicy
  trustPolicy?: TrustPolicy
  trustPolicyExclude?: PackageVersionPolicy
  trustPolicyIgnoreAfter?: number
}

export type FastRewriteOptions = ResolverPolicyOptions & {
  lockfileDir: string
  lockfileIncludeTarballUrl?: boolean
  isLockfileUpToDate: (lockfile: LockfileObject) => Promise<boolean>
  readPackageHook?: ReadPackageHook
  registriesByScope: RegistriesByScope
  registryOptionsByUrl?: Record<string, RegistryOptions>
  requestPackage: RequestPackageFunction
  verifyLockfile?: (lockfile: LockfileObject) => Promise<void>
}

export async function tryFastUpdateOverrides (
  lockfile: LockfileObject,
  opts: FastRewriteOptions & {
    overrides: Record<string, string>
    parsedOverrides: VersionOverride[]
  }
): Promise<boolean> {
  const fastOverrides = getFastOverrides(lockfile, lockfile.overrides ?? {}, opts.overrides, opts.parsedOverrides)
  if (fastOverrides == null) return false
  if (movesACatalogedPackage(lockfile, fastOverrides)) return false
  return applyFastRewrite(lockfile, fastOverrides, opts, { overrides: opts.overrides })
}

/**
 * Whether any of `fastOverrides` moves a package a catalog entry records the
 * version of. The entry would have to move with it — which is the catalog
 * rewrite's job, not this one's — so the move goes to the resolver instead.
 *
 * A `parent>child` selector names no importer edge, and only importer edges
 * are what a catalog entry resolves.
 */
function movesACatalogedPackage (lockfile: LockfileObject, fastOverrides: FastOverride[]): boolean {
  const catalogedAliases = new Set(
    Object.values(lockfile.catalogs ?? {}).flatMap((catalog) => Object.keys(catalog))
  )
  return fastOverrides.some(({ name, parent }) => parent == null && catalogedAliases.has(name))
}

/**
 * Move every package named by `fastOverrides` to its new version, rebuilding
 * the affected package entries from the new version's manifest and redirecting
 * everything that referenced the old key. `settings` is the setting block that
 * drove the rewrite, recorded alongside it.
 *
 * Returns `false` whenever the move cannot be proven safe from the lockfile
 * plus the resolved manifests — a locked child the new manifest no longer
 * admits, or a candidate the freshness check rejects.
 */
export async function applyFastRewrite (
  lockfile: LockfileObject,
  fastOverrides: FastOverride[],
  opts: FastRewriteOptions,
  settings: Partial<LockfileObject>
): Promise<boolean> {
  const removals = fastOverrides.filter(({ newVersion }) => newVersion == null)
  const moves = fastOverrides.filter(({ newVersion }) => newVersion != null)
  const peerNames = getPeerNames(lockfile)
  if (removals.some(({ name }) => peerNames.has(name))) return false

  const replacements = collectReplacements(lockfile, fastOverrides)
  if (replacements == null) return false

  const changedNames = new Set(
    fastOverrides
      .filter(({ newVersion }) => newVersion != null)
      .map(({ name }) => name)
  )
  const rewriteContext = { changedNames, peerNames, removals, replacements, moves }
  const manifests = await resolveNewManifests(fastOverrides, replacements, opts)
  if (manifests == null) return false

  const packages = rewritePackages(lockfile.packages ?? {}, {
    lockfileIncludeTarballUrl: opts.lockfileIncludeTarballUrl,
    manifests,
    registriesByScope: opts.registriesByScope,
    registryOptionsByUrl: opts.registryOptionsByUrl,
    rewriteContext,
  })
  if (packages == null) return false

  const importers = Object.fromEntries(
    Object.entries(lockfile.importers).map(([id, importer]) => [
      id,
      {
        ...importer,
        dependencies: rewriteResolvedDependencies(importer.dependencies, rewriteContext),
        devDependencies: rewriteResolvedDependencies(importer.devDependencies, rewriteContext),
        optionalDependencies: rewriteResolvedDependencies(importer.optionalDependencies, rewriteContext),
      },
    ])
  ) as LockfileObject['importers']

  const updatedLockfile: LockfileObject = {
    ...lockfile,
    importers,
    packages: pruneUnreachablePackages(importers, packages),
    ...settings,
  }
  if (!await opts.isLockfileUpToDate(updatedLockfile)) return false
  await opts.verifyLockfile?.(updatedLockfile)
  lockfile.importers = updatedLockfile.importers
  lockfile.packages = updatedLockfile.packages
  Object.assign(lockfile, settings)
  return true
}

function getFastOverrides (
  lockfile: LockfileObject,
  oldOverrides: Record<string, string>,
  newOverrides: Record<string, string>,
  parsedOverrides: VersionOverride[]
): FastOverride[] | null {
  if (Object.keys(oldOverrides).some((selector) => newOverrides[selector] == null)) return null

  const changedSelectors = Object.keys(newOverrides)
    .filter((selector) => oldOverrides[selector] !== newOverrides[selector])
  if (changedSelectors.length === 0) return null

  const parsedBySelector = new Map(parsedOverrides.map((override) => [override.selector, override]))
  const changedNames = new Set<string>()
  const result: FastOverride[] = []
  for (const selector of changedSelectors) {
    const override = parsedBySelector.get(selector)
    const newValue = newOverrides[selector]
    const oldVersion = oldOverrides[selector]
    const removesDependency = newValue === '-'
    if (
      override == null ||
      override.targetPkg.bareSpecifier != null ||
      override.converge === true ||
      !removesDependency && (
        overriddenVersion(lockfile, override.targetPkg.name, newValue) == null ||
        oldVersion != null && semver.valid(oldVersion) == null
      ) ||
      changedNames.has(override.targetPkg.name) ||
      parsedOverrides.some((candidate) =>
        candidate.selector !== selector &&
        candidate.targetPkg.name === override.targetPkg.name
      )
    ) {
      return null
    }
    changedNames.add(override.targetPkg.name)
    result.push({
      name: override.targetPkg.name,
      ...override.parentPkg == null
        ? {}
        : {
          parent: {
            name: override.parentPkg.name,
            bareSpecifier: override.parentPkg.bareSpecifier,
          },
        },
      ...removesDependency
        ? {}
        : {
          newVersion: overriddenVersion(lockfile, override.targetPkg.name, newValue)!,
          oldVersion,
        },
    })
  }
  return result
}

/**
 * The version an override moves its target to.
 *
 * A range names the highest already-locked version satisfying it, because
 * `preferredVersions` makes the resolver reuse a version the graph already
 * holds rather than the highest published.
 */
function overriddenVersion (
  lockfile: LockfileObject,
  name: string,
  value: string
): string | null {
  if (semver.valid(value) != null) return value
  if (semver.validRange(value) == null) return null
  const versions: string[] = []
  for (const [depPath, snapshot] of Object.entries(lockfile.packages ?? {})) {
    const parsed = nameVerFromPkgSnapshot(depPath, snapshot)
    if (parsed.name !== name || parsed.nonSemverVersion != null) continue
    if (parsed.registryName != null || dp.parseDepPath(depPath).peerDepGraphHash !== '') return null
    if (semver.valid(parsed.version) != null && semver.satisfies(parsed.version, value)) {
      versions.push(parsed.version)
    }
  }
  return versions.sort(semver.rcompare)[0] ?? null
}

function collectReplacements (
  lockfile: LockfileObject,
  overrides: FastOverride[]
): Map<DepPath, DepPath> | null {
  const overridesByName = new Map(overrides.map((override) => [override.name, override]))
  const replacements = new Map<DepPath, DepPath>()
  for (const dependencies of allResolvedDependencyMaps(lockfile)) {
    for (const [alias, reference] of Object.entries(dependencies)) {
      const override = overridesByName.get(alias)
      if (override?.newVersion == null) continue
      const oldDepPath = dp.refToRelative(reference, alias)
      if (oldDepPath == null) return null
      const parsed = dp.parse(oldDepPath)
      const snapshot = lockfile.packages?.[oldDepPath]
      if (
        parsed.name !== alias ||
        parsed.version == null ||
        // The fast path rebuilds the dep path as `<alias>@<version>`, which
        // would drop the registry qualifier of a named-registry package.
        parsed.registryName != null ||
        parsed.peerDepGraphHash != null ||
        parsed.patchHash != null ||
        override.oldVersion != null && parsed.version !== override.oldVersion ||
        snapshot == null ||
        snapshot.optional === true ||
        snapshot.peerDependencies != null ||
        snapshot.peerDependenciesMeta != null ||
        !('integrity' in snapshot.resolution) ||
        typeof snapshot.resolution.integrity !== 'string' ||
        'type' in snapshot.resolution && snapshot.resolution.type != null
      ) {
        return null
      }
      const newDepPath = `${alias}@${override.newVersion}${parsed.peerDepGraphHash ?? ''}` as DepPath
      const previousReplacement = replacements.get(oldDepPath)
      if (previousReplacement != null && previousReplacement !== newDepPath) return null
      replacements.set(oldDepPath, newDepPath)
    }
  }
  for (const dependencies of allResolvedDependencyMaps(lockfile)) {
    for (const [alias, reference] of Object.entries(dependencies)) {
      const depPath = dp.refToRelative(reference, alias)
      if (depPath != null && replacements.has(depPath) && !overridesByName.has(alias)) return null
    }
  }
  return replacements
}

function getPeerNames (lockfile: LockfileObject): Set<string> {
  const result = new Set<string>()
  for (const snapshot of Object.values(lockfile.packages ?? {})) {
    for (const name of Object.keys(snapshot.peerDependencies ?? {})) result.add(name)
    for (const name of Object.keys(snapshot.peerDependenciesMeta ?? {})) result.add(name)
    for (const name of snapshot.transitivePeerDependencies ?? []) result.add(name)
  }
  return result
}

function allResolvedDependencyMaps (lockfile: LockfileObject): ResolvedDependencies[] {
  const result: ResolvedDependencies[] = []
  for (const importer of Object.values(lockfile.importers)) {
    if (importer.dependencies != null) result.push(importer.dependencies)
    if (importer.devDependencies != null) result.push(importer.devDependencies)
    if (importer.optionalDependencies != null) result.push(importer.optionalDependencies)
  }
  for (const snapshot of Object.values(lockfile.packages ?? {})) {
    if (snapshot.dependencies != null) result.push(snapshot.dependencies)
    if (snapshot.optionalDependencies != null) result.push(snapshot.optionalDependencies)
  }
  return result
}

async function resolveNewManifests (
  overrides: FastOverride[],
  replacements: Map<DepPath, DepPath>,
  opts: ResolverPolicyOptions & {
    lockfileDir: string
    readPackageHook?: ReadPackageHook
    requestPackage: RequestPackageFunction
  }
): Promise<Map<string, {
  manifest: PackageManifest
  resolution: Awaited<ReturnType<RequestPackageFunction>>['body']['resolution']
}> | null> {
  const changedNames = new Set(
    [...replacements]
      .filter(([oldDepPath, newDepPath]) => oldDepPath !== newDepPath)
      .map(([oldDepPath]) => dp.parse(oldDepPath).name!)
  )
  const results = await Promise.all(overrides.map(async ({ name, newVersion }) => {
    if (newVersion == null) return null
    if (!changedNames.has(name)) return null
    const response = await opts.requestPackage({
      alias: name,
      bareSpecifier: newVersion,
    }, {
      downloadPriority: 0,
      lockfileDir: opts.lockfileDir,
      preferredVersions: Object.create(null),
      projectDir: opts.lockfileDir,
      publishedBy: opts.publishedBy,
      publishedByExclude: opts.publishedByExclude,
      skipFetch: true,
      trustPolicy: opts.trustPolicy,
      trustPolicyExclude: opts.trustPolicyExclude,
      trustPolicyIgnoreAfter: opts.trustPolicyIgnoreAfter,
      update: false,
    })
    if (
      response.body.isLocal ||
      response.body.manifest == null ||
      response.body.policyViolation != null ||
      response.body.resolvedVia !== 'npm-registry' ||
      response.resolutionNeedsFetch === true ||
      !('integrity' in response.body.resolution) ||
      typeof response.body.resolution.integrity !== 'string' ||
      response.body.resolution.type != null
    ) {
      return undefined
    }
    const rawManifest = response.body.manifest
    if (
      rawManifest.name !== name ||
      rawManifest.version !== newVersion ||
      rawManifest.deprecated != null ||
      hasInvalidManifestMaps(rawManifest) ||
      hasPeerDependencies(rawManifest) ||
      rawManifest.engines?.runtime != null ||
      rawManifest.bundledDependencies != null ||
      rawManifest.bundleDependencies != null
    ) {
      return undefined
    }
    const manifest = opts.readPackageHook == null
      ? rawManifest
      : await opts.readPackageHook(clone(rawManifest))
    // pacquet validates after its manifest hook, so a hook that introduces
    // any of these must send both stacks down the same fallback.
    if (
      manifest.name !== name ||
      manifest.version !== newVersion ||
      manifest.deprecated != null ||
      hasInvalidManifestMaps(manifest) ||
      hasPeerDependencies(manifest) ||
      manifest.engines?.runtime != null ||
      manifest.bundledDependencies != null ||
      manifest.bundleDependencies != null
    ) return undefined
    return {
      name,
      manifest,
      resolution: response.body.resolution,
    }
  }))
  if (results.some((result) => result === undefined)) return null
  return new Map(results
    .filter((result) => result != null)
    .map(({ name, ...value }) => [name, value]))
}

function hasInvalidManifestMaps (manifest: PackageManifest): boolean {
  return [
    manifest.dependencies,
    manifest.optionalDependencies,
    manifest.peerDependencies,
    manifest.peerDependenciesMeta,
    manifest.engines,
  ].some((value) =>
    value != null &&
    (typeof value !== 'object' || Array.isArray(value))
  )
}

function hasPeerDependencies (manifest: PackageManifest): boolean {
  return Object.keys(manifest.peerDependencies ?? {}).length > 0 ||
    Object.keys(manifest.peerDependenciesMeta ?? {}).length > 0
}

function rewritePackages (
  originalPackages: Record<DepPath, PackageSnapshot>,
  opts: {
    lockfileIncludeTarballUrl?: boolean
    manifests: Map<string, {
      manifest: PackageManifest
      resolution: Awaited<ReturnType<RequestPackageFunction>>['body']['resolution']
    }>
    registriesByScope: RegistriesByScope
    registryOptionsByUrl?: Record<string, RegistryOptions>
    rewriteContext: RewriteContext
  }
): Record<DepPath, PackageSnapshot> | null {
  const packages = Object.fromEntries(
    Object.entries(originalPackages).map(([depPath, snapshot]) => [
      depPath,
      {
        ...snapshot,
        dependencies: rewriteResolvedDependencies(snapshot.dependencies, opts.rewriteContext, depPath as DepPath),
        optionalDependencies: rewriteResolvedDependencies(snapshot.optionalDependencies, opts.rewriteContext, depPath as DepPath),
      },
    ])
  ) as Record<DepPath, PackageSnapshot>

  for (const [oldDepPath, newDepPath] of opts.rewriteContext.replacements) {
    if (oldDepPath === newDepPath) continue
    const oldSnapshot = originalPackages[oldDepPath]
    const name = dp.parse(oldDepPath).name!
    const resolved = opts.manifests.get(name)
    if (resolved == null) return null
    const dependencies = validateAndRewriteDependencies({
      lockedDependencies: oldSnapshot.dependencies,
      manifestDependencies: effectiveDependencies(resolved.manifest),
      packages: originalPackages,
      parentDepPath: newDepPath,
      rewriteContext: opts.rewriteContext,
    })
    const optionalDependencies = validateAndRewriteDependencies({
      lockedDependencies: oldSnapshot.optionalDependencies,
      manifestDependencies: resolved.manifest.optionalDependencies,
      packages: originalPackages,
      parentDepPath: newDepPath,
      rewriteContext: opts.rewriteContext,
    })
    if (dependencies === null || optionalDependencies === null) return null

    const registry = dp.getRegistryByPackageName(opts.registriesByScope, name)
    const newSnapshot = createPackageSnapshot(oldSnapshot, {
      dependencies,
      lockfileIncludeTarballUrl: opts.lockfileIncludeTarballUrl,
      manifest: resolved.manifest,
      optionalDependencies,
      registry,
      serverType: getRegistryServerType(opts, registry),
      resolution: resolved.resolution,
    })
    const existingSnapshot = packages[newDepPath]
    if (existingSnapshot == null) {
      packages[newDepPath] = newSnapshot
    } else {
      if (!equals(existingSnapshot, newSnapshot)) return null
      packages[newDepPath] = newSnapshot
    }
  }
  return packages
}

function effectiveDependencies (manifest: PackageManifest): Record<string, string> | undefined {
  if (manifest.dependencies == null) return undefined
  const optionalNames = new Set(Object.keys(manifest.optionalDependencies ?? {}))
  return Object.fromEntries(
    Object.entries(manifest.dependencies).filter(([name]) => !optionalNames.has(name))
  )
}

function validateAndRewriteDependencies (opts: {
  lockedDependencies: ResolvedDependencies | undefined
  manifestDependencies: Record<string, string> | undefined
  packages: Record<DepPath, PackageSnapshot>
  parentDepPath: DepPath
  rewriteContext: RewriteContext
}): ResolvedDependencies | undefined | null {
  const { lockedDependencies, manifestDependencies, packages, parentDepPath, rewriteContext } = opts
  const manifestEntries = Object.entries(manifestDependencies ?? {})
  for (const name of Object.keys(lockedDependencies ?? {})) {
    if (manifestDependencies?.[name] == null && rewriteContext.peerNames.has(name)) return null
  }
  const result: ResolvedDependencies = {}
  for (const [name, range] of manifestEntries) {
    if (shouldRemoveDependency(name, parentDepPath, rewriteContext.removals)) continue
    if (semver.validRange(range) == null) return null
    const lockedReference = lockedDependencies?.[name]
    const reference = lockedReference == null
      ? findReusableReference({ name, packages, range, rewriteContext })
      : rewriteReference(name, lockedReference, rewriteContext, parentDepPath)
    if (reference == null) return null
    const depPath = dp.refToRelative(reference, name)
    const version = depPath == null ? null : dp.parse(depPath).version
    if (version == null || !semver.satisfies(version, range)) return null
    result[name] = reference
  }
  return Object.keys(result).length === 0 ? undefined : result
}

function findReusableReference (opts: {
  name: string
  packages: Record<DepPath, PackageSnapshot>
  range: string
  rewriteContext: RewriteContext
}): string | undefined {
  const { name, packages, range, rewriteContext } = opts
  if (
    rewriteContext.changedNames.has(name) ||
    rewriteContext.peerNames.has(name) ||
    rewriteContext.removals.some((removal) => removal.name === name)
  ) {
    return undefined
  }
  const candidates = Object.entries(packages)
    .filter(([depPath, snapshot]) => {
      const parsed = dp.parse(depPath as DepPath)
      return parsed.name === name &&
        parsed.version != null &&
        parsed.peerDepGraphHash == null &&
        parsed.patchHash == null &&
        semver.satisfies(parsed.version, range) &&
        snapshot.optional !== true &&
        snapshot.id == null &&
        snapshot.peerDependencies == null &&
        snapshot.peerDependenciesMeta == null &&
        snapshot.transitivePeerDependencies == null &&
        'integrity' in snapshot.resolution &&
        typeof snapshot.resolution.integrity === 'string' &&
        (!('type' in snapshot.resolution) || snapshot.resolution.type == null)
    })
    .map(([depPath]) => depPath as DepPath)
  if (candidates.length !== 1) return undefined
  return candidates[0].startsWith(`${name}@`)
    ? candidates[0].substring(name.length + 1)
    : candidates[0]
}

function createPackageSnapshot (
  oldSnapshot: PackageSnapshot,
  opts: {
    dependencies?: ResolvedDependencies
    lockfileIncludeTarballUrl?: boolean
    manifest: PackageManifest
    optionalDependencies?: ResolvedDependencies
    registry: string
    serverType?: RegistryServerType
    resolution: Awaited<ReturnType<RequestPackageFunction>>['body']['resolution']
  }
): PackageSnapshot {
  const snapshot: PackageSnapshot = {
    resolution: toLockfileResolution({
      name: opts.manifest.name,
      version: opts.manifest.version,
    }, opts.resolution, {
      registry: opts.registry,
      serverType: opts.serverType,
      lockfileIncludeTarballUrl: opts.lockfileIncludeTarballUrl,
    }),
  }
  if (opts.dependencies != null) snapshot.dependencies = opts.dependencies
  if (opts.optionalDependencies != null) snapshot.optionalDependencies = opts.optionalDependencies
  if (oldSnapshot.optional === true) snapshot.optional = true
  if (oldSnapshot.transitivePeerDependencies != null) {
    snapshot.transitivePeerDependencies = oldSnapshot.transitivePeerDependencies
  }
  if (opts.manifest.engines != null) {
    const engines = Object.fromEntries(
      Object.entries(opts.manifest.engines).filter(([, range]) => range !== '*')
    )
    if (Object.keys(engines).length > 0) snapshot.engines = engines as PackageSnapshot['engines']
  }
  if (opts.manifest.cpu != null) snapshot.cpu = opts.manifest.cpu
  if (opts.manifest.os != null) snapshot.os = opts.manifest.os
  if (opts.manifest.libc != null) snapshot.libc = opts.manifest.libc
  if (opts.manifest.deprecated) snapshot.deprecated = opts.manifest.deprecated
  if (opts.manifest.bin && !(opts.manifest.bin === '' || Object.keys(opts.manifest.bin).length === 0) || opts.manifest.directories?.bin) {
    snapshot.hasBin = true
  }
  return snapshot
}

function rewriteResolvedDependencies (
  dependencies: ResolvedDependencies | undefined,
  rewriteContext: RewriteContext,
  parentDepPath?: DepPath
): ResolvedDependencies | undefined {
  if (dependencies == null) return undefined
  const rewritten = Object.fromEntries(
    Object.entries(dependencies)
      .filter(([alias]) => !shouldRemoveDependency(alias, parentDepPath, rewriteContext.removals))
      .map(([alias, reference]) => [
        alias,
        rewriteReference(alias, reference, rewriteContext, parentDepPath),
      ])
  )
  return Object.keys(rewritten).length === 0 ? undefined : rewritten
}

function shouldRemoveDependency (
  alias: string,
  parentDepPath: DepPath | undefined,
  removals: FastOverride[]
): boolean {
  return overrideApplies(alias, parentDepPath, removals)
}

/**
 * Whether any of `overrides` names an edge on `alias` owned by
 * `parentDepPath`. A selector without a parent names every edge; one with a
 * parent names only edges out of a package it matches, which an importer
 * never is.
 */
function overrideApplies (
  alias: string,
  parentDepPath: DepPath | undefined,
  overrides: FastOverride[]
): boolean {
  return overrides.some((override) => {
    if (override.name !== alias) return false
    if (override.parent == null) return true
    if (parentDepPath == null) return false
    const parent = dp.parse(parentDepPath)
    return parent.name === override.parent.name &&
      parent.version != null &&
      (
        override.parent.bareSpecifier == null ||
        semver.satisfies(parent.version, override.parent.bareSpecifier)
      )
  })
}

function rewriteReference (
  alias: string,
  reference: string,
  { changedNames, replacements, moves }: RewriteContext,
  parentDepPath?: DepPath
): string {
  if (!changedNames.has(alias)) return reference
  // A `parent>child` selector names only the edges out of that parent, so
  // the other dependents keep the version they have.
  if (!overrideApplies(alias, parentDepPath, moves)) return reference
  const oldDepPath = dp.refToRelative(reference, alias)
  if (oldDepPath == null) return reference
  const newDepPath = replacements.get(oldDepPath)
  if (newDepPath == null) return reference
  return newDepPath.startsWith(`${alias}@`)
    ? newDepPath.substring(alias.length + 1)
    : newDepPath
}

function pruneUnreachablePackages (
  importers: LockfileObject['importers'],
  packages: Record<DepPath, PackageSnapshot>
): Record<DepPath, PackageSnapshot> {
  const reachable = new Set<DepPath>()
  const queue: DepPath[] = []
  for (const importer of Object.values(importers)) {
    enqueueDependencies(importer.dependencies)
    enqueueDependencies(importer.devDependencies)
    enqueueDependencies(importer.optionalDependencies)
  }
  for (let index = 0; index < queue.length; index++) {
    const depPath = queue[index]
    const snapshot = packages[depPath]
    if (snapshot == null) continue
    enqueueDependencies(snapshot.dependencies)
    enqueueDependencies(snapshot.optionalDependencies)
  }
  return Object.fromEntries(
    Object.entries(packages).filter(([depPath]) => reachable.has(depPath as DepPath))
  ) as Record<DepPath, PackageSnapshot>

  function enqueueDependencies (dependencies: ResolvedDependencies | undefined): void {
    for (const [alias, reference] of Object.entries(dependencies ?? {})) {
      const depPath = dp.refToRelative(reference, alias)
      if (depPath == null || reachable.has(depPath)) continue
      reachable.add(depPath)
      queue.push(depPath)
    }
  }
}
