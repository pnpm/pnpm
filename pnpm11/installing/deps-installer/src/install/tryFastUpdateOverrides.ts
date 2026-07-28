import type { VersionOverride } from '@pnpm/config.parse-overrides'
import * as dp from '@pnpm/deps.path'
import type {
  LockfileObject,
  PackageSnapshot,
  ResolvedDependencies,
} from '@pnpm/lockfile.types'
import { toLockfileResolution } from '@pnpm/lockfile.utils'
import type { RequestPackageFunction } from '@pnpm/store.controller-types'
import type {
  DepPath,
  PackageManifest,
  ReadPackageHook,
  Registries,
} from '@pnpm/types'
import { clone, equals } from 'ramda'
import semver from 'semver'

interface FastOverride {
  name: string
  newVersion?: string
  oldVersion?: string
}

interface RewriteContext {
  changedNames: Set<string>
  removedNames: Set<string>
  replacements: Map<DepPath, DepPath>
}

export async function tryFastUpdateOverrides (
  lockfile: LockfileObject,
  opts: {
    lockfileDir: string
    lockfileIncludeTarballUrl?: boolean
    overrides: Record<string, string>
    parsedOverrides: VersionOverride[]
    isLockfileUpToDate: (lockfile: LockfileObject) => Promise<boolean>
    readPackageHook?: ReadPackageHook
    registries: Registries
    requestPackage: RequestPackageFunction
    verifyLockfile?: (lockfile: LockfileObject) => Promise<void>
  }
): Promise<boolean> {
  const fastOverrides = getFastOverrides(lockfile.overrides ?? {}, opts.overrides, opts.parsedOverrides)
  if (fastOverrides == null) return false

  const removedNames = new Set(
    fastOverrides
      .filter(({ newVersion }) => newVersion == null)
      .map(({ name }) => name)
  )
  if ([...removedNames].some((name) => isUsedAsPeer(lockfile, name))) return false

  const replacements = collectReplacements(lockfile, fastOverrides)
  if (replacements == null) return false

  const changedNames = new Set(
    fastOverrides
      .filter(({ newVersion }) => newVersion != null)
      .map(({ name }) => name)
  )
  const rewriteContext = { changedNames, removedNames, replacements }
  const manifests = await resolveNewManifests(fastOverrides, replacements, opts)
  if (manifests == null) return false

  const packages = rewritePackages(lockfile.packages ?? {}, {
    lockfileIncludeTarballUrl: opts.lockfileIncludeTarballUrl,
    manifests,
    registries: opts.registries,
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
    overrides: opts.overrides,
  }
  if (!await opts.isLockfileUpToDate(updatedLockfile)) return false
  await opts.verifyLockfile?.(updatedLockfile)
  lockfile.importers = updatedLockfile.importers
  lockfile.packages = updatedLockfile.packages
  lockfile.overrides = updatedLockfile.overrides
  return true
}

function getFastOverrides (
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
  let removesDependencies: boolean | undefined
  for (const selector of changedSelectors) {
    const override = parsedBySelector.get(selector)
    const newValue = newOverrides[selector]
    const oldVersion = oldOverrides[selector]
    const removesDependency = newValue === '-'
    if (
      override == null ||
      override.parentPkg != null ||
      override.targetPkg.bareSpecifier != null ||
      override.converge === true ||
      !removesDependency && (
        semver.valid(newValue) == null ||
        oldVersion != null && semver.valid(oldVersion) == null
      ) ||
      removesDependencies != null && removesDependencies !== removesDependency ||
      changedNames.has(override.targetPkg.name) ||
      parsedOverrides.some((candidate) =>
        candidate.selector !== selector &&
        candidate.targetPkg.name === override.targetPkg.name
      )
    ) {
      return null
    }
    removesDependencies = removesDependency
    changedNames.add(override.targetPkg.name)
    result.push({
      name: override.targetPkg.name,
      ...removesDependency ? {} : { newVersion: newValue, oldVersion },
    })
  }
  return result
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

function isUsedAsPeer (lockfile: LockfileObject, name: string): boolean {
  return Object.values(lockfile.packages ?? {}).some((snapshot) =>
    snapshot.peerDependencies?.[name] != null ||
    snapshot.peerDependenciesMeta?.[name] != null ||
    snapshot.transitivePeerDependencies?.includes(name) === true
  )
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
  opts: {
    lockfileDir: string
    parsedOverrides: VersionOverride[]
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
      skipFetch: true,
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
    if (hasPeerDependencies(manifest)) return undefined
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
    registries: Registries
    rewriteContext: RewriteContext
  }
): Record<DepPath, PackageSnapshot> | null {
  const packages = Object.fromEntries(
    Object.entries(originalPackages).map(([depPath, snapshot]) => [
      depPath,
      {
        ...snapshot,
        dependencies: rewriteResolvedDependencies(snapshot.dependencies, opts.rewriteContext),
        optionalDependencies: rewriteResolvedDependencies(snapshot.optionalDependencies, opts.rewriteContext),
      },
    ])
  ) as Record<DepPath, PackageSnapshot>

  for (const [oldDepPath, newDepPath] of opts.rewriteContext.replacements) {
    if (oldDepPath === newDepPath) continue
    const oldSnapshot = originalPackages[oldDepPath]
    const name = dp.parse(oldDepPath).name!
    const resolved = opts.manifests.get(name)
    if (resolved == null) return null
    const dependencies = validateAndRewriteDependencies(
      effectiveDependencies(resolved.manifest),
      oldSnapshot.dependencies,
      opts.rewriteContext
    )
    const optionalDependencies = validateAndRewriteDependencies(
      resolved.manifest.optionalDependencies,
      oldSnapshot.optionalDependencies,
      opts.rewriteContext
    )
    if (dependencies === null || optionalDependencies === null) return null

    const newSnapshot = createPackageSnapshot(oldSnapshot, {
      dependencies,
      lockfileIncludeTarballUrl: opts.lockfileIncludeTarballUrl,
      manifest: resolved.manifest,
      optionalDependencies,
      registry: dp.getRegistryByPackageName(opts.registries, name),
      resolution: resolved.resolution,
    })
    const existingSnapshot = packages[newDepPath]
    if (existingSnapshot == null) {
      packages[newDepPath] = newSnapshot
    } else {
      const mergedSnapshot = mergeEquivalentSnapshots(existingSnapshot, newSnapshot)
      if (mergedSnapshot == null) return null
      packages[newDepPath] = mergedSnapshot
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

function validateAndRewriteDependencies (
  manifestDependencies: Record<string, string> | undefined,
  lockedDependencies: ResolvedDependencies | undefined,
  rewriteContext: RewriteContext
): ResolvedDependencies | undefined | null {
  const manifestEntries = Object.entries(manifestDependencies ?? {})
  const lockedEntries = Object.entries(lockedDependencies ?? {})
  if (
    manifestEntries.length !== lockedEntries.length ||
    manifestEntries.some(([name]) => lockedDependencies?.[name] == null)
  ) {
    return null
  }
  const result: ResolvedDependencies = {}
  for (const [name, range] of manifestEntries) {
    if (semver.validRange(range) == null) return null
    const reference = rewriteReference(name, lockedDependencies![name], rewriteContext)
    const depPath = dp.refToRelative(reference, name)
    const version = depPath == null ? null : dp.parse(depPath).version
    if (version == null || !semver.satisfies(version, range)) return null
    result[name] = reference
  }
  return Object.keys(result).length === 0 ? undefined : result
}

function createPackageSnapshot (
  oldSnapshot: PackageSnapshot,
  opts: {
    dependencies?: ResolvedDependencies
    lockfileIncludeTarballUrl?: boolean
    manifest: PackageManifest
    optionalDependencies?: ResolvedDependencies
    registry: string
    resolution: Awaited<ReturnType<RequestPackageFunction>>['body']['resolution']
  }
): PackageSnapshot {
  const snapshot: PackageSnapshot = {
    resolution: toLockfileResolution({
      name: opts.manifest.name,
      version: opts.manifest.version,
    }, opts.resolution, opts.registry, opts.lockfileIncludeTarballUrl),
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

function mergeEquivalentSnapshots (
  first: PackageSnapshot,
  second: PackageSnapshot
): PackageSnapshot | null {
  const { optional: firstOptional, ...firstComparable } = first
  const { optional: secondOptional, ...secondComparable } = second
  if (!equals(firstComparable, secondComparable)) return null
  return {
    ...firstComparable,
    ...firstOptional === true && secondOptional === true ? { optional: true } : {},
  }
}

function rewriteResolvedDependencies (
  dependencies: ResolvedDependencies | undefined,
  rewriteContext: RewriteContext
): ResolvedDependencies | undefined {
  if (dependencies == null) return undefined
  const rewritten = Object.fromEntries(
    Object.entries(dependencies)
      .filter(([alias]) => !rewriteContext.removedNames.has(alias))
      .map(([alias, reference]) => [
        alias,
        rewriteReference(alias, reference, rewriteContext),
      ])
  )
  return Object.keys(rewritten).length === 0 ? undefined : rewritten
}

function rewriteReference (
  alias: string,
  reference: string,
  { changedNames, replacements }: RewriteContext
): string {
  if (!changedNames.has(alias)) return reference
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
