import {
  type CatalogResolutionFound,
  matchCatalogResolveResult,
  resolveFromCatalog,
  type WantedDependency,
} from '@pnpm/catalogs.resolver'
import type { Catalogs } from '@pnpm/catalogs.types'
import { createMatcher } from '@pnpm/config.matcher'
import { parseOverrides } from '@pnpm/config.parse-overrides'
import { LOCKFILE_VERSION } from '@pnpm/constants'
import * as dp from '@pnpm/deps.path'
import { PnpmError } from '@pnpm/error'
import { createReadPackageHook } from '@pnpm/hooks.read-package-hook'
import type { ResolveLatestDispatcher } from '@pnpm/installing.client'
import {
  getLockfileImporterId,
  type LockfileObject,
  type ProjectSnapshot,
} from '@pnpm/lockfile.fs'
import { getAllDependenciesFromManifest, getDependencyTypeFromManifest } from '@pnpm/pkg-manifest.utils'
import {
  DEPENDENCIES_FIELDS,
  type DependenciesField,
  type DependenciesOrPeersField,
  type DepPath,
  type IncludedDependencies,
  type PackageManifest,
  type PackageVersionPolicy,
  type ProjectId,
  type ProjectManifest,
} from '@pnpm/types'
import semver from 'semver'

export * from './createManifestGetter.js'

export interface OutdatedPackage {
  alias: string
  belongsTo: DependenciesOrPeersField
  current?: string // not defined means the package is not installed
  latestManifest?: PackageManifest
  packageName: string
  wanted: string
  workspace?: string
}

export interface OutdatedOptions {
  catalogs?: Catalogs
  compatible?: boolean
  currentLockfile: LockfileObject | null
  resolveLatest: ResolveLatestDispatcher
  ignoreDependencies?: string[]
  include?: IncludedDependencies
  lockfileDir: string
  manifest: ProjectManifest
  match?: (dependencyName: string) => boolean
  minimumReleaseAge?: number
  minimumReleaseAgeExclude?: string[]
  prefix: string
  publishedBy?: Date
  publishedByExclude?: PackageVersionPolicy
  wantedLockfile: LockfileObject | null
}

interface OutdatedContext {
  opts: OutdatedOptions
  importerId: ProjectId
  workspace: string
  currentLockfile: LockfileObject
  allDeps: Record<string, string>
  overriddenManifest: ProjectManifest
  ignoreDependenciesMatcher?: (name: string) => boolean
  includePeerDependencies: boolean
  replaceCatalog: (dep: WantedDependency) => string | undefined
  resolveOpts: Parameters<ResolveLatestDispatcher>[1]
}

export async function outdated (opts: OutdatedOptions): Promise<OutdatedPackage[]> {
  const includePeerDependencies = opts.include?.peerDependencies === true
  if (packageHasNoDeps(opts.manifest, includePeerDependencies)) return []
  if (opts.wantedLockfile == null) {
    throw new PnpmError('OUTDATED_NO_LOCKFILE', `No lockfile in directory "${opts.lockfileDir}". Run \`pnpm install\` to generate one.`)
  }

  const ctx = await createOutdatedContext(opts, includePeerDependencies)
  const dependencyTypes: DependenciesOrPeersField[] = includePeerDependencies
    ? [...DEPENDENCIES_FIELDS, 'peerDependencies']
    : DEPENDENCIES_FIELDS

  const results = await Promise.all(
    dependencyTypes.map((depType) => processDependencyType(ctx, depType))
  )

  const outdated = results.flat()
  return outdated.sort((pkg1, pkg2) => pkg1.packageName.localeCompare(pkg2.packageName))
}

async function createOutdatedContext (
  opts: OutdatedOptions,
  includePeerDependencies: boolean
): Promise<OutdatedContext> {
  const overriddenManifest = await getOverriddenManifest(opts)
  const allDeps = getAllDependenciesFromManifest(overriddenManifest, {
    autoInstallPeers: includePeerDependencies,
  })
  const importerId = getLockfileImporterId(opts.lockfileDir, opts.prefix)
  const workspace = opts.manifest.name?.trim() || importerId
  const currentLockfile: LockfileObject = opts.currentLockfile ?? {
    lockfileVersion: LOCKFILE_VERSION,
    importers: { [importerId]: { specifiers: {} } },
  }

  return {
    opts,
    importerId,
    workspace,
    currentLockfile,
    allDeps,
    overriddenManifest,
    ignoreDependenciesMatcher: opts.ignoreDependencies?.length ? createMatcher(opts.ignoreDependencies) : undefined,
    includePeerDependencies,
    replaceCatalog: replaceCatalogProtocolIfNecessary.bind(null, opts.catalogs ?? {}),
    resolveOpts: {
      lockfileDir: opts.lockfileDir,
      preferredVersions: {},
      projectDir: opts.prefix,
      publishedBy: opts.publishedBy,
      publishedByExclude: opts.publishedByExclude,
    },
  }
}

async function getOverriddenManifest (opts: OutdatedOptions): Promise<ProjectManifest> {
  const overrides = opts.currentLockfile?.overrides ?? opts.wantedLockfile?.overrides
  if (overrides) {
    const readPackageHook = createReadPackageHook({
      lockfileDir: opts.lockfileDir,
      overrides: parseOverrides(overrides, opts.catalogs ?? {}),
    })
    const manifest = await readPackageHook?.(opts.manifest, opts.lockfileDir)
    if (manifest) return manifest
  }
  return opts.manifest
}

async function processDependencyType (
  ctx: OutdatedContext,
  depType: DependenciesOrPeersField
): Promise<OutdatedPackage[]> {
  if (ctx.opts.include?.[depType] === false) return []

  const declaredDependencies = depType === 'peerDependencies'
    ? ctx.overriddenManifest.peerDependencies
    : ctx.opts.wantedLockfile!.importers[ctx.importerId][depType]
  if (declaredDependencies == null) return []

  let pkgs = Object.keys(declaredDependencies)
  if (ctx.opts.match != null) {
    pkgs = pkgs.filter((pkgName) => ctx.opts.match!(pkgName))
  }

  const results = await Promise.all(
    pkgs.map((alias) => checkDependencyAlias(ctx, depType, declaredDependencies, alias))
  )
  return results.filter((pkg): pkg is OutdatedPackage => pkg != null)
}

function shouldSkipAlias (
  ctx: OutdatedContext,
  depType: DependenciesOrPeersField,
  alias: string
): boolean {
  if (
    ctx.includePeerDependencies &&
    depType !== 'peerDependencies' &&
    ctx.opts.manifest.peerDependencies?.[alias] != null &&
    ctx.opts.manifest[depType]?.[alias] == null
  ) return true
  if (ctx.ignoreDependenciesMatcher?.(alias)) return true
  return false
}

async function checkDependencyAlias (
  ctx: OutdatedContext,
  depType: DependenciesOrPeersField,
  declaredDependencies: Record<string, string>,
  alias: string
): Promise<OutdatedPackage | undefined> {
  if (shouldSkipAlias(ctx, depType, alias)) return undefined

  const declaredSpecifier = depType === 'peerDependencies'
    ? declaredDependencies[alias]
    : ctx.allDeps[alias]
  if (!declaredSpecifier) return undefined

  const manifestDepType = depType === 'peerDependencies'
    ? getDependencyTypeFromManifest(ctx.opts.manifest, alias)
    : depType
  const lockfileDepType: DependenciesField = manifestDepType === 'peerDependencies' || manifestDepType == null
    ? 'dependencies'
    : manifestDepType
  const wantedRef = ctx.opts.wantedLockfile!.importers[ctx.importerId][lockfileDepType]?.[alias] ?? declaredSpecifier
  if (isLocalRef(wantedRef)) return undefined

  const currentRef = (ctx.currentLockfile.importers[ctx.importerId] as ProjectSnapshot)?.[lockfileDepType]?.[alias]
  return resolveOutdatedPackage({
    ctx,
    depType,
    alias,
    declaredSpecifier,
    wantedRef,
    currentRef,
  })
}

interface ResolvePackageContext {
  ctx: OutdatedContext
  depType: DependenciesOrPeersField
  alias: string
  declaredSpecifier: string
  wantedRef: string
  currentRef?: string
}

interface PackageDisplayVersions {
  packageName: string
  wanted: string
  current?: string
}

function getPackageDisplayVersions (
  ctx: OutdatedContext,
  alias: string,
  wantedRef: string,
  currentRef?: string
): PackageDisplayVersions {
  const wantedRelative = dp.refToRelative(wantedRef, alias)
  const currentRelative = currentRef ? dp.refToRelative(currentRef, alias) : null
  const wantedSnapshot = wantedRelative != null ? ctx.opts.wantedLockfile!.packages?.[wantedRelative] : undefined
  const currentSnapshot = currentRelative != null ? ctx.currentLockfile.packages?.[currentRelative] : undefined
  const packageName = (wantedRelative != null ? dp.parse(wantedRelative).name : undefined) ?? alias
  const wanted = displayVersion(wantedRef, wantedRelative, wantedSnapshot?.version)
  const current = currentRef ? displayVersion(currentRef, currentRelative, currentSnapshot?.version) : undefined
  return { packageName, wanted, current }
}

async function resolveOutdatedPackage (params: ResolvePackageContext): Promise<OutdatedPackage | undefined> {
  const { ctx, depType, alias, declaredSpecifier, wantedRef, currentRef } = params
  const bareSpecifier = ctx.replaceCatalog({ alias, bareSpecifier: declaredSpecifier })
  const info = await ctx.opts.resolveLatest(
    { wantedDependency: { alias, bareSpecifier }, compatible: ctx.opts.compatible },
    ctx.resolveOpts
  )
  if (info == null) return undefined

  const { packageName, wanted, current } = getPackageDisplayVersions(ctx, alias, wantedRef, currentRef)
  return determineOutdatedPackage({
    alias,
    depType,
    current,
    latestManifest: info.latestManifest,
    packageName,
    wanted,
    workspace: ctx.workspace,
  })
}

interface DetermineOutdatedParams {
  alias: string
  depType: DependenciesOrPeersField
  current?: string
  latestManifest?: PackageManifest
  packageName: string
  wanted: string
  workspace: string
}

function determineOutdatedPackage (params: DetermineOutdatedParams): OutdatedPackage | undefined {
  const { alias, depType, current, latestManifest, packageName, wanted, workspace } = params
  if (latestManifest == null) {
    if (wanted !== current) {
      return { alias, belongsTo: depType, current, latestManifest: undefined, packageName, wanted, workspace }
    }
    return undefined
  }
  if (!current) {
    return { alias, belongsTo: depType, latestManifest, packageName, wanted, workspace }
  }
  if (wanted !== current || isLowerVersion(wanted, latestManifest.version) || latestManifest.deprecated) {
    return { alias, belongsTo: depType, current, latestManifest, packageName, wanted, workspace }
  }
  return undefined
}

function packageHasNoDeps (manifest: ProjectManifest, includePeerDependencies: boolean): boolean {
  return ((manifest.dependencies == null) || isEmpty(manifest.dependencies)) &&
    ((manifest.devDependencies == null) || isEmpty(manifest.devDependencies)) &&
    ((manifest.optionalDependencies == null) || isEmpty(manifest.optionalDependencies)) &&
    (!includePeerDependencies || manifest.peerDependencies == null || isEmpty(manifest.peerDependencies))
}

function isEmpty (obj: object): boolean {
  return Object.keys(obj).length === 0
}

// A dependency whose wanted ref is local resolves to a directory on disk
// even when its manifest specifier is a plain semver range (e.g. a
// workspace package matched by `link-workspace-packages`). Such a package
// may not be published at all, so there is no registry "latest" to compare
// against.
function isLocalRef (ref: string): boolean {
  return ref.startsWith('link:') || ref.startsWith('file:') || ref.startsWith('workspace:')
}

// Pick a clean display string for a lockfile ref.
//
//   - If the dep-path parses to a semver, that's the value (handles
//     `pkg@1.0.0(peer-hash)` and aliased `positive: is-positive@3.1.0`).
//   - If the dep-path's non-semver version contains a `/`, it's a
//     URL/git-shape (`https://`, `git+ssh://`, scheme-less `github.com/.../sha`,
//     `link:../foo`, etc.) — return the raw ref so a commit/path change is
//     visible to the user.
//   - Otherwise prefer `snapshot.version` (clean semver for `runtime:`-style
//     refs); fall back to the raw ref when the snapshot didn't record one.
function displayVersion (ref: string, relativeDepPath: DepPath | null, snapshotVersion: string | undefined): string {
  if (relativeDepPath != null) {
    const parsed = dp.parse(relativeDepPath)
    if (parsed.version != null) return parsed.version
    if (parsed.nonSemverVersion?.includes('/')) return ref
  }
  return snapshotVersion ?? ref
}

// semver.lt throws on non-semver strings (e.g. URL refs from git/tarball).
// Treat those as "not lower" so a ref change still gets surfaced via the
// `wantedRef !== currentRef` check above.
function isLowerVersion (current: string, latest: string): boolean {
  if (!semver.valid(current) || !semver.valid(latest)) return false
  return semver.lt(current, latest)
}

function replaceCatalogProtocolIfNecessary (catalogs: Catalogs, wantedDependency: WantedDependency) {
  return matchCatalogResolveResult(resolveFromCatalog(catalogs, wantedDependency), {
    unused: () => wantedDependency.bareSpecifier,
    found: (found: CatalogResolutionFound) => found.resolution.specifier,
    misconfiguration: (misconfiguration) => {
      throw misconfiguration.error
    },
  })
}
