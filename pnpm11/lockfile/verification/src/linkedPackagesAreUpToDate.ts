import path from 'node:path'

import { refToRelative, removeSuffix } from '@pnpm/deps.path'
import type {
  PackageSnapshot,
  PackageSnapshots,
  ProjectSnapshot,
} from '@pnpm/lockfile.types'
import { refIsLocalDirectory } from '@pnpm/lockfile.utils'
import { safeReadPackageJsonFromDir } from '@pnpm/pkg-manifest.reader'
import type { DirectoryResolution, WorkspacePackages } from '@pnpm/resolving.resolver-base'
import {
  DEPENDENCIES_FIELDS,
  type DependencyManifest,
  type ProjectManifest,
} from '@pnpm/types'
import getVersionSelectorType from 'version-selector-type'

import {
  getTargetPkgName,
  getVersionRange,
  isSubdirectory,
  isWorkspacePath,
  isWorkspaceRangeSpec,
  resolveSpecPath,
  versionSatisfies,
} from './dependencySpec.js'
import { localDirectoryDepIsUpToDate } from './localDirectoryDepIsUpToDate.js'

export interface CheckLinkedPackagesResult {
  upToDate: boolean
  detailedReason?: string
}

export interface LinkedPackagesContext {
  linkWorkspacePackages: boolean
  manifestsByDir: Record<string, DependencyManifest>
  workspacePackages?: WorkspacePackages
  lockfilePackages?: PackageSnapshots
  lockfileDir: string
  workspaceDir?: string
  injectWorkspacePackages?: boolean
  /**
   * Skips comparing the dependencies of local directory dependencies with
   * their lockfile snapshots. That comparison errs toward reporting a stale
   * lockfile, which only costs a re-resolve, so a caller that fails on a
   * stale verdict skips it.
   */
  skipLocalDirectoryDependencies?: boolean
}

interface ProjectToCheck {
  dir: string
  manifest: ProjectManifest
  snapshot: ProjectSnapshot
}

interface DependencyToCheck {
  depName: string
  currentSpec: string
  lockfileRef: string
}

const UP_TO_DATE: CheckLinkedPackagesResult = { upToDate: true }

export async function linkedPackagesAreUpToDate (ctx: LinkedPackagesContext, project: ProjectToCheck): Promise<boolean> {
  const result = await checkLinkedPackagesAreUpToDate(ctx, project)
  return result.upToDate
}

export async function checkLinkedPackagesAreUpToDate (ctx: LinkedPackagesContext, project: ProjectToCheck): Promise<CheckLinkedPackagesResult> {
  const results = await Promise.all(DEPENDENCIES_FIELDS.flatMap((depField) => {
    const lockfileDeps = project.snapshot[depField]
    const manifestDeps = project.manifest[depField]
    if ((lockfileDeps == null) || (manifestDeps == null)) return []
    return Object.entries(lockfileDeps)
      .filter(([depName]) => manifestDeps[depName])
      .map(async ([depName, lockfileRef]) => checkDependency(ctx, project, { depName, currentSpec: manifestDeps[depName], lockfileRef }))
  }))
  return results.find((result) => !result.upToDate) ?? UP_TO_DATE
}

async function checkDependency (
  ctx: LinkedPackagesContext,
  project: ProjectToCheck,
  dep: DependencyToCheck
): Promise<CheckLinkedPackagesResult> {
  if (refIsLocalDirectory(project.snapshot.specifiers[dep.depName]) || isLocalDirectoryRef(dep.lockfileRef)) {
    return checkLocalDirectoryDependency(ctx, project, dep)
  }
  if (dep.lockfileRef.startsWith('link:')) {
    const specPath = getLinkSpecPath(dep.currentSpec)
    if (specPath != null) {
      return checkLinkTarget(project.dir, dep, specPath)
    }
  }
  return checkWorkspaceLink(ctx, project, dep)
}

async function checkLocalDirectoryDependency (
  ctx: LinkedPackagesContext,
  project: ProjectToCheck,
  { depName, currentSpec, lockfileRef }: DependencyToCheck
): Promise<CheckLinkedPackagesResult> {
  // When a file: specifier resolves to link: in the lockfile
  // (e.g. injected self-references), it's a local link with no
  // entry in the packages section. Treat it as up-to-date.
  if (lockfileRef.startsWith('link:')) return UP_TO_DATE
  const isWorkspaceRange = isWorkspaceRangeSpec(currentSpec)
  if (isWorkspaceRange && !isInjected(ctx, project, depName)) {
    return outdated(`Workspace dependency "${depName}" is not injected, but the lockfile resolves it to "${lockfileRef}"`)
  }
  const depPath = refToRelative(lockfileRef, depName)
  const pkgSnapshot = depPath == null ? undefined : ctx.lockfilePackages?.[depPath]
  if (pkgSnapshot == null) {
    return outdated(`The lockfile has no package entry for local directory dependency "${depName}" (${lockfileRef})`)
  }
  if (!ctx.skipLocalDirectoryDependencies && !await localDirectoryDepIsUpToDate(ctx, pkgSnapshot)) {
    return outdated(`The dependencies of local directory dependency "${depName}" do not match the lockfile`)
  }
  return isWorkspaceRange
    ? checkInjectedWorkspacePackage(ctx, { depName, currentSpec, pkgSnapshot })
    : UP_TO_DATE
}

/**
 * The path a `link:`, `file:`, or `workspace:<path>` specifier points to.
 */
function getLinkSpecPath (currentSpec: string): string | undefined {
  if (currentSpec.startsWith('link:') || currentSpec.startsWith('file:')) {
    return currentSpec.slice(5)
  }
  if (currentSpec.startsWith('workspace:')) {
    const target = currentSpec.slice(10)
    if (isWorkspacePath(target)) return target
  }
  return undefined
}

function checkLinkTarget (
  projectDir: string,
  { depName, currentSpec, lockfileRef }: DependencyToCheck,
  specPath: string
): CheckLinkedPackagesResult {
  return resolveSpecPath(projectDir, specPath) === resolveSpecPath(projectDir, lockfileRef.slice(5))
    ? UP_TO_DATE
    : outdated(`Dependency "${depName}" is linked to "${lockfileRef}", which does not match "${currentSpec}"`)
}

async function checkWorkspaceLink (
  ctx: LinkedPackagesContext,
  project: ProjectToCheck,
  dep: DependencyToCheck
): Promise<CheckLinkedPackagesResult> {
  const { depName, currentSpec, lockfileRef } = dep
  const isLinked = lockfileRef.startsWith('link:')
  const pkgName = getTargetPkgName(currentSpec, depName)
  const availableRange = getVersionRange(currentSpec)
  // https://github.com/pnpm/pnpm/issues/6592
  // if the dependency is linked and the specified version type is tag, we consider it to be up-to-date to skip full resolution.
  if (isLinked && getVersionSelectorType(availableRange)?.type === 'tag') {
    return UP_TO_DATE
  }
  const workspacePackagesWithName = ctx.workspacePackages?.get(pkgName)
  const linkedDir = isLinked
    ? path.join(project.dir, lockfileRef.slice(5))
    : workspacePackagesWithName?.get(removeSuffix(lockfileRef))?.rootDir
  if (!linkedDir) {
    return checkWorkspaceRangeIsNotLinked(dep, { pkgName, availableRange, workspacePackagesWithName })
  }
  if (!ctx.linkWorkspacePackages && !currentSpec.startsWith('workspace:')) {
    // we found a linked dir, but we don't want to use it, because it's not specified as a
    // workspace:x.x.x dependency
    return UP_TO_DATE
  }
  if (
    isLinked &&
    isWorkspaceRangeSpec(currentSpec) &&
    workspacePackagesWithName != null &&
    // The link may point to the package's publishConfig.directory.
    !isInsideAnyWorkspacePackage(workspacePackagesWithName, linkedDir)
  ) {
    return outdated(`Workspace dependency "${depName}" is linked to "${linkedDir}", which is not inside a workspace package named "${pkgName}"`)
  }
  return checkLinkedPackageVersion(ctx, dep, { isLinked, linkedDir, availableRange })
}

type WorkspacePackagesWithName = ReturnType<WorkspacePackages['get']>

function checkWorkspaceRangeIsNotLinked (
  { currentSpec }: DependencyToCheck,
  { pkgName, availableRange, workspacePackagesWithName }: {
    pkgName: string
    availableRange: string
    workspacePackagesWithName: WorkspacePackagesWithName
  }
): CheckLinkedPackagesResult {
  const satisfyingWorkspacePackage = isWorkspaceRangeSpec(currentSpec) && workspacePackagesWithName
    ? Array.from(workspacePackagesWithName.values()).find(({ manifest }) => versionSatisfies(manifest.version, availableRange))
    : undefined
  if (satisfyingWorkspacePackage != null) {
    return outdated(`Workspace package "${pkgName}" (${satisfyingWorkspacePackage.manifest.version}) satisfies range "${currentSpec}" but is not linked in the lockfile`)
  }
  return UP_TO_DATE
}

function isInsideAnyWorkspacePackage (workspacePackagesWithName: NonNullable<WorkspacePackagesWithName>, linkedDir: string): boolean {
  return Array.from(workspacePackagesWithName.values()).some(({ rootDir }) => isSubdirectory(path.resolve(rootDir), path.resolve(linkedDir)))
}

async function checkLinkedPackageVersion (
  ctx: LinkedPackagesContext,
  { depName, currentSpec }: DependencyToCheck,
  { isLinked, linkedDir, availableRange }: { isLinked: boolean, linkedDir: string, availableRange: string }
): Promise<CheckLinkedPackagesResult> {
  const linkedPkg = ctx.manifestsByDir[linkedDir] ?? await safeReadPackageJsonFromDir(linkedDir)
  const localPackageSatisfiesRange = versionSatisfies(linkedPkg?.version, availableRange)
  if (isLinked === localPackageSatisfiesRange) return UP_TO_DATE
  return outdated(isLinked
    ? `Linked workspace package "${depName}" (${linkedPkg?.version ?? 'unknown'}) does not satisfy range "${currentSpec}"`
    : `Workspace package "${depName}" (${linkedPkg?.version ?? 'unknown'}) satisfies range "${currentSpec}" but is not linked in the lockfile`)
}

async function checkInjectedWorkspacePackage (
  ctx: LinkedPackagesContext,
  { depName, currentSpec, pkgSnapshot }: { depName: string, currentSpec: string, pkgSnapshot: PackageSnapshot }
): Promise<CheckLinkedPackagesResult> {
  const range = getVersionRange(currentSpec)
  if (!('directory' in (pkgSnapshot.resolution ?? {})) || getVersionSelectorType(range)?.type === 'tag') {
    return UP_TO_DATE
  }
  const injectedDir = path.join(ctx.lockfileDir, (pkgSnapshot.resolution as DirectoryResolution).directory)
  const injectedPkg = ctx.manifestsByDir[injectedDir] ?? await safeReadPackageJsonFromDir(injectedDir)
  const expectedName = getTargetPkgName(currentSpec, depName)
  if (injectedPkg?.name !== expectedName) {
    return outdated(`Injected workspace dependency "${depName}" resolves to "${injectedDir}", which is not the package "${expectedName}"`)
  }
  if (!versionSatisfies(injectedPkg.version, range)) {
    return outdated(`Injected workspace package "${depName}" (${injectedPkg.version}) does not satisfy range "${currentSpec}"`)
  }
  return UP_TO_DATE
}

function isInjected (ctx: LinkedPackagesContext, project: ProjectToCheck, depName: string): boolean {
  return Boolean(
    ctx.injectWorkspacePackages ||
    project.manifest.dependenciesMeta?.[depName]?.injected ||
    project.snapshot.dependenciesMeta?.[depName]?.injected
  )
}

/**
 * Also matches aliased refs such as `foo@file:packages/foo`, which injected
 * workspace dependencies installed under another name have.
 */
function isLocalDirectoryRef (ref: string): boolean {
  if (refIsLocalDirectory(ref)) return true
  const refWithoutSuffix = removeSuffix(ref)
  const aliasEnd = refWithoutSuffix.indexOf('@file:', 1)
  return aliasEnd !== -1 && refIsLocalDirectory(refWithoutSuffix.slice(aliasEnd + 1))
}

function outdated (detailedReason: string): CheckLinkedPackagesResult {
  return { upToDate: false, detailedReason }
}
