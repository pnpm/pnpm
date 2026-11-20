import { getRegistryServerType } from '@pnpm/config.normalize-registries'
import * as dp from '@pnpm/deps.path'
import type {
  PackageSnapshot,
  ResolvedDependencies,
} from '@pnpm/lockfile.types'
import { toLockfileResolution } from '@pnpm/lockfile.utils'
import type {
  DepPath,
  PackageManifest,
  RegistriesByScope,
  RegistryOptions,
  RegistryServerType,
} from '@pnpm/types'
import { equals } from 'ramda'
import semver from 'semver'

import type { RegistryResolution, ResolvedManifest } from './fastRewriteManifests.js'

export interface FastOverride {
  name: string
  newVersion?: string
  oldVersion?: string
  parent?: {
    name: string
    bareSpecifier?: string
  }
}

export interface RewriteContext {
  changedNames: Set<string>
  peerNames: Set<string>
  removals: FastOverride[]
  replacements: Map<DepPath, DepPath>
  /** The replacing overrides, needed to scope a `parent>child` selector. */
  moves: FastOverride[]
}

interface RewritePackagesOptions {
  lockfileIncludeTarballUrl?: boolean
  manifests: Map<string, ResolvedManifest>
  registriesByScope: RegistriesByScope
  registryOptionsByUrl?: Record<string, RegistryOptions>
  rewriteContext: RewriteContext
}

export function rewritePackages (
  originalPackages: Record<DepPath, PackageSnapshot>,
  opts: RewritePackagesOptions
): Record<DepPath, PackageSnapshot> | null {
  const packages = rewritePackageEdges(originalPackages, opts.rewriteContext)
  for (const [oldDepPath, newDepPath] of opts.rewriteContext.replacements) {
    if (oldDepPath === newDepPath) continue
    const newSnapshot = createReplacementSnapshot(originalPackages, { oldDepPath, newDepPath }, opts)
    if (newSnapshot == null) return null
    const existingSnapshot = packages[newDepPath]
    if (existingSnapshot != null && !equals(existingSnapshot, newSnapshot)) return null
    packages[newDepPath] = newSnapshot
  }
  return packages
}

function rewritePackageEdges (
  originalPackages: Record<DepPath, PackageSnapshot>,
  rewriteContext: RewriteContext
): Record<DepPath, PackageSnapshot> {
  return Object.fromEntries(
    Object.entries(originalPackages).map(([depPath, snapshot]) => [
      depPath,
      {
        ...snapshot,
        dependencies: rewriteResolvedDependencies(snapshot.dependencies, rewriteContext, depPath as DepPath),
        optionalDependencies: rewriteResolvedDependencies(snapshot.optionalDependencies, rewriteContext, depPath as DepPath),
      },
    ])
  ) as Record<DepPath, PackageSnapshot>
}

/**
 * The snapshot the package at `oldDepPath` gets once moved to `newDepPath`,
 * built from the new version's manifest, or `null` when a locked child no
 * longer fits it.
 */
function createReplacementSnapshot (
  originalPackages: Record<DepPath, PackageSnapshot>,
  { oldDepPath, newDepPath }: { oldDepPath: DepPath, newDepPath: DepPath },
  opts: RewritePackagesOptions
): PackageSnapshot | null {
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
  return createPackageSnapshot(oldSnapshot, {
    dependencies,
    lockfileIncludeTarballUrl: opts.lockfileIncludeTarballUrl,
    manifest: resolved.manifest,
    optionalDependencies,
    registry,
    serverType: getRegistryServerType(opts, registry),
    resolution: resolved.resolution,
  })
}

function effectiveDependencies (manifest: PackageManifest): Record<string, string> | undefined {
  if (manifest.dependencies == null) return undefined
  const optionalNames = new Set(Object.keys(manifest.optionalDependencies ?? {}))
  return Object.fromEntries(
    Object.entries(manifest.dependencies).filter(([name]) => !optionalNames.has(name))
  )
}

interface DependencyValidationOptions {
  lockedDependencies: ResolvedDependencies | undefined
  manifestDependencies: Record<string, string> | undefined
  packages: Record<DepPath, PackageSnapshot>
  parentDepPath: DepPath
  rewriteContext: RewriteContext
}

function validateAndRewriteDependencies (opts: DependencyValidationOptions): ResolvedDependencies | undefined | null {
  const { manifestDependencies, parentDepPath, rewriteContext } = opts
  const manifestEntries = Object.entries(manifestDependencies ?? {})
  if (dropsLockedPeer(opts.lockedDependencies, manifestDependencies, rewriteContext.peerNames)) return null
  const result: ResolvedDependencies = {}
  for (const [name, range] of manifestEntries) {
    if (shouldRemoveDependency(name, parentDepPath, rewriteContext.removals)) continue
    const reference = satisfyingReference(name, range, opts)
    if (reference == null) return null
    result[name] = reference
  }
  return Object.keys(result).length === 0 ? undefined : result
}

function dropsLockedPeer (
  lockedDependencies: ResolvedDependencies | undefined,
  manifestDependencies: Record<string, string> | undefined,
  peerNames: Set<string>
): boolean {
  return Object.keys(lockedDependencies ?? {})
    .some((name) => manifestDependencies?.[name] == null && peerNames.has(name))
}

/**
 * The reference the new version's dependency on `name` gets: the locked one,
 * rewritten, or a single reusable package already in the lockfile. `null`
 * when it does not satisfy `range`.
 */
function satisfyingReference (
  name: string,
  range: string,
  { lockedDependencies, packages, parentDepPath, rewriteContext }: DependencyValidationOptions
): string | null {
  if (semver.validRange(range) == null) return null
  const lockedReference = lockedDependencies?.[name]
  const reference = lockedReference == null
    ? findReusableReference({ name, packages, range, rewriteContext })
    : rewriteReference(name, lockedReference, rewriteContext, parentDepPath)
  if (reference == null) return null
  const depPath = dp.refToRelative(reference, name)
  const version = depPath == null ? null : dp.parse(depPath).version
  if (version == null || !semver.satisfies(version, range)) return null
  return reference
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
    resolution: RegistryResolution
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
  copyManifestFields(snapshot, opts.manifest)
  return snapshot
}

function copyManifestFields (snapshot: PackageSnapshot, manifest: PackageManifest): void {
  const engines = constrainingEngines(manifest)
  if (engines != null) snapshot.engines = engines
  if (manifest.cpu != null) snapshot.cpu = manifest.cpu
  if (manifest.os != null) snapshot.os = manifest.os
  if (manifest.libc != null) snapshot.libc = manifest.libc
  if (manifest.deprecated) snapshot.deprecated = manifest.deprecated
  if (declaresBin(manifest)) {
    snapshot.hasBin = true
  }
}

/** The manifest's `engines` without the `*` entries, or `undefined` when none is left. */
function constrainingEngines (manifest: PackageManifest): PackageSnapshot['engines'] | undefined {
  if (manifest.engines == null) return undefined
  const engines = Object.fromEntries(
    Object.entries(manifest.engines).filter(([, range]) => range !== '*')
  )
  return Object.keys(engines).length > 0 ? engines as PackageSnapshot['engines'] : undefined
}

function declaresBin (manifest: PackageManifest): boolean {
  return Boolean(
    manifest.bin && !(manifest.bin === '' || Object.keys(manifest.bin).length === 0) || manifest.directories?.bin
  )
}

export function rewriteResolvedDependencies (
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
