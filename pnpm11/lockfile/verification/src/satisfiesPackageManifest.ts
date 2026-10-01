import { createMatcher } from '@pnpm/config.matcher'
import * as dp from '@pnpm/deps.path'
import type { ProjectSnapshot } from '@pnpm/lockfile.types'
import {
  DEPENDENCIES_FIELDS,
  type DependenciesField,
  type ProjectManifest,
} from '@pnpm/types'
import { equals, omit, pickBy } from 'ramda'
import semver from 'semver'

import { type Diff, diffFlatRecords, isEqual } from './diffFlatRecords.js'
import { dependencySpecifiersAreEqual } from './gitSpecifiersAreEquivalent.js'
import { unresolvedOptionalDependencies } from './unresolvedOptionalDependencies.js'

export interface SatisfiesPackageManifestOptions {
  autoInstallPeers?: boolean
  excludeLinksFromLockfile?: boolean
  ignoredOptionalDependencies?: string[]
  /**
   * Treat an `optionalDependencies` entry the importer has no entry for as
   * satisfied: the install that wrote the lockfile could not resolve it
   * and skipped it. Only a frozen install may assume this; a resolving
   * install retries the dependency instead.
   */
  allowUnresolvedOptionalDependencies?: boolean
}

interface SatisfiesPackageManifestResult {
  satisfies: boolean
  detailedReason?: string
}

export function satisfiesPackageManifest (
  opts: SatisfiesPackageManifestOptions,
  importer: ProjectSnapshot | undefined,
  pkg: ProjectManifest
): SatisfiesPackageManifestResult {
  if (!importer) return { satisfies: false, detailedReason: 'no importer' }
  const ignoredOptionalDependencies = getIgnoredOptionalDependencies(opts, importer, pkg)
  const { manifest, existingDeps } = getExpectedDependencies(opts, pkg, ignoredOptionalDependencies)
  const specs = opts?.excludeLinksFromLockfile ? pickNonLinkedDeps(importer.specifiers) : importer.specifiers
  const specsDiff = diffFlatRecords(specs, existingDeps, dependencySpecifiersAreEqual)
  if (!isEqual(specsDiff)) {
    return {
      satisfies: false,
      detailedReason: `specifiers in the lockfile don't match specifiers in package.json:\n${displaySpecDiff(specsDiff)}`,
    }
  }
  return findPublishDirectoryMismatch(importer, manifest) ??
    findDependenciesMetaMismatch(importer, manifest) ??
    findDependencyFieldsMismatch({
      excludeLinksFromLockfile: opts?.excludeLinksFromLockfile,
      ignoredOptionalDependencies,
      importer,
      manifest,
    }) ??
    { satisfies: true }
}

function getIgnoredOptionalDependencies (
  opts: SatisfiesPackageManifestOptions,
  importer: ProjectSnapshot,
  pkg: ProjectManifest
): Set<string> {
  const ignoredOptionalDependencies = new Set(
    opts.ignoredOptionalDependencies?.length
      ? Object.keys(pkg.optionalDependencies ?? {})
        .filter(createMatcher(opts.ignoredOptionalDependencies))
      : []
  )
  if (opts.allowUnresolvedOptionalDependencies) {
    for (const depName of Object.keys(unresolvedOptionalDependencies(opts, importer, pkg))) {
      ignoredOptionalDependencies.add(depName)
    }
  }
  return ignoredOptionalDependencies
}

/**
 * The manifest as the lockfile should reflect it, with auto-installed peers
 * added to its dependencies, and the specifiers the lockfile should record.
 */
function getExpectedDependencies (
  opts: SatisfiesPackageManifestOptions,
  pkg: ProjectManifest,
  ignoredOptionalDependencies: Set<string>
): { manifest: ProjectManifest, existingDeps: Record<string, string> } {
  let manifest = pkg
  let existingDeps = omitIgnoredDependencies(
    { ...pkg.devDependencies, ...pkg.dependencies, ...pkg.optionalDependencies },
    ignoredOptionalDependencies
  )
  if (opts?.autoInstallPeers) {
    manifest = {
      ...pkg,
      dependencies: {
        ...pkg.peerDependencies && omit(Object.keys(existingDeps), pkg.peerDependencies),
        ...pkg.dependencies,
      },
    }
    existingDeps = {
      ...manifest.peerDependencies,
      ...existingDeps,
    }
  }
  if (opts?.excludeLinksFromLockfile) {
    existingDeps = pickNonLinkedDeps(existingDeps)
  }
  return { manifest, existingDeps }
}

function findPublishDirectoryMismatch (
  importer: ProjectSnapshot,
  pkg: ProjectManifest
): SatisfiesPackageManifestResult | undefined {
  if (importer.publishDirectory !== pkg.publishConfig?.directory) {
    return {
      satisfies: false,
      detailedReason: `"publishDirectory" in the lockfile (${importer.publishDirectory ?? 'undefined'}) doesn't match "publishConfig.directory" in package.json (${pkg.publishConfig?.directory ?? 'undefined'})`,
    }
  }
  const lockfileLinksPublishDirectory = importer.publishDirectory != null && importer.linkDirectory !== false
  const manifestLinksPublishDirectory = pkg.publishConfig?.directory != null && pkg.publishConfig.linkDirectory !== false
  if (lockfileLinksPublishDirectory !== manifestLinksPublishDirectory) {
    return {
      satisfies: false,
      detailedReason: `"linkDirectory" in the lockfile (${lockfileLinksPublishDirectory}) doesn't match "publishConfig.linkDirectory" in package.json (${manifestLinksPublishDirectory})`,
    }
  }
  return undefined
}

function findDependenciesMetaMismatch (
  importer: ProjectSnapshot,
  pkg: ProjectManifest
): SatisfiesPackageManifestResult | undefined {
  if (equals(pkg.dependenciesMeta ?? {}, importer.dependenciesMeta ?? {})) return undefined
  return {
    satisfies: false,
    detailedReason: `importer dependencies meta (${JSON.stringify(importer.dependenciesMeta)}) doesn't match package manifest dependencies meta (${JSON.stringify(pkg.dependenciesMeta)})`,
  }
}

interface DependencyFieldsContext {
  excludeLinksFromLockfile?: boolean
  ignoredOptionalDependencies: Set<string>
  importer: ProjectSnapshot
  manifest: ProjectManifest
}

function findDependencyFieldsMismatch (ctx: DependencyFieldsContext): SatisfiesPackageManifestResult | undefined {
  for (const depField of DEPENDENCIES_FIELDS) {
    const mismatch = findDependencyFieldMismatch(ctx, depField)
    if (mismatch) return mismatch
  }
  return undefined
}

function findDependencyFieldMismatch (
  ctx: DependencyFieldsContext,
  depField: DependenciesField
): SatisfiesPackageManifestResult | undefined {
  const importerDeps = ctx.importer[depField] ?? {}
  const pkgDeps = getManifestDepsOfField(ctx, depField)
  const pkgDepNames = getDepNamesOfField(ctx.manifest, depField, pkgDeps)
  if (
    pkgDepNames.length !== Object.keys(importerDeps).length &&
    pkgDepNames.length !== countOfNonLinkedDeps(importerDeps)
  ) {
    return {
      satisfies: false,
      detailedReason: `"${depField}" in the lockfile (${JSON.stringify(importerDeps)}) doesn't match the same field in package.json (${JSON.stringify(pkgDeps)})`,
    }
  }
  for (const depName of pkgDepNames) {
    const mismatch = findDependencyMismatch(ctx.importer, { depField, depName, importerDeps, pkgDeps })
    if (mismatch) return mismatch
  }
  return undefined
}

function getManifestDepsOfField (ctx: DependencyFieldsContext, depField: DependenciesField): Record<string, string> {
  const pkgDeps = depField === 'devDependencies'
    ? ctx.manifest[depField] ?? {}
    : omitIgnoredDependencies(ctx.manifest[depField], ctx.ignoredOptionalDependencies)
  return ctx.excludeLinksFromLockfile ? pickNonLinkedDeps(pkgDeps) : pkgDeps
}

/**
 * The names of the dependencies in `pkgDeps` that belong to `depField`, as a
 * dependency listed in several fields belongs to the one that wins at install.
 */
function getDepNamesOfField (pkg: ProjectManifest, depField: DependenciesField, pkgDeps: Record<string, string>): string[] {
  switch (depField) {
    case 'optionalDependencies':
      return Object.keys(pkgDeps)
    case 'devDependencies':
      return Object.keys(pkgDeps)
        .filter((depName) => !pkg.optionalDependencies?.[depName] && !pkg.dependencies?.[depName])
    case 'dependencies':
      return Object.keys(pkgDeps)
        .filter((depName) => !pkg.optionalDependencies?.[depName])
    default:
      throw new Error(`Unknown dependency type "${depField as string}"`)
  }
}

function findDependencyMismatch (
  importer: ProjectSnapshot,
  { depField, depName, importerDeps, pkgDeps }: {
    depField: DependenciesField
    depName: string
    importerDeps: Record<string, string>
    pkgDeps: Record<string, string>
  }
): SatisfiesPackageManifestResult | undefined {
  if (!importerDeps[depName] || !dependencySpecifiersAreEqual(importer.specifiers?.[depName], pkgDeps[depName])) {
    return {
      satisfies: false,
      detailedReason: `importer ${depField}.${depName} specifier ${importer.specifiers[depName]} don't match package manifest specifier (${pkgDeps[depName]})`,
    }
  }
  const range = importer.specifiers[depName]
  if (range == null || !semver.validRange(range)) return undefined
  const version = dp.removeSuffix(importerDeps[depName])
  if (semver.valid(version) && !semver.satisfies(version, range)) {
    return {
      satisfies: false,
      detailedReason: `The importer resolution is broken at dependency "${depName}": version "${version}" doesn't satisfy range "${range}"`,
    }
  }
  return undefined
}

function omitIgnoredDependencies (
  dependencies: Record<string, string> | undefined,
  ignoredDependencies: Set<string>
): Record<string, string> {
  const filteredDependencies = { ...dependencies }
  for (const dependency of ignoredDependencies) {
    delete filteredDependencies[dependency]
  }
  return filteredDependencies
}

function pickNonLinkedDeps (deps: Record<string, string>): Record<string, string> {
  return pickBy((spec: string) => !spec.startsWith('link:'), deps)
}

function countOfNonLinkedDeps (lockfileDeps: { [depName: string]: string }): number {
  return Object.values(lockfileDeps).filter((ref) => !ref.includes('link:') && !ref.includes('file:')).length
}

function displaySpecDiff ({ added, removed, modified }: Diff<string, string>): string {
  let result = ''

  if (added.length !== 0) {
    result += `* ${added.length} dependencies were added: `
    result += added.map(({ key, value }) => `${key}@${value}`).join(', ')
    result += '\n'
  }

  if (removed.length !== 0) {
    result += `* ${removed.length} dependencies were removed: `
    result += removed.map(({ key, value }) => `${key}@${value}`).join(', ')
    result += '\n'
  }

  if (modified.length !== 0) {
    result += `* ${modified.length} dependencies are mismatched:\n`
    for (const { key, left, right } of modified) {
      result += `  - ${key} (lockfile: ${left}, manifest: ${right})\n`
    }
  }

  return result
}
