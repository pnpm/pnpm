import fs from 'node:fs'
import os from 'node:os'
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
  DEPENDENCIES_OR_PEER_FIELDS,
  type DependencyManifest,
  type ProjectManifest,
} from '@pnpm/types'
import pEvery from 'p-every'
import semver from 'semver'
import getVersionSelectorType from 'version-selector-type'

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
  { depName, currentSpec, lockfileRef }: DependencyToCheck
): Promise<CheckLinkedPackagesResult> {
  const isWorkspaceRange = currentSpec.startsWith('workspace:') && !isWorkspacePath(currentSpec.slice(10))
  if (refIsLocalDirectory(project.snapshot.specifiers[depName]) || isLocalDirectoryRef(lockfileRef)) {
    // When a file: specifier resolves to link: in the lockfile
    // (e.g. injected self-references), it's a local link with no
    // entry in the packages section. Treat it as up-to-date.
    if (lockfileRef.startsWith('link:')) return UP_TO_DATE
    if (isWorkspaceRange && !isInjected(ctx, project, depName)) {
      return outdated(`Workspace dependency "${depName}" is not injected, but the lockfile resolves it to "${lockfileRef}"`)
    }
    const depPath = refToRelative(lockfileRef, depName)
    const pkgSnapshot = depPath == null ? undefined : ctx.lockfilePackages?.[depPath]
    if (!await isLocalFileDepUpdated(ctx.lockfileDir, pkgSnapshot, ctx.manifestsByDir, ctx.workspaceDir)) {
      return outdated(`The dependencies of local directory dependency "${depName}" do not match the lockfile`)
    }
    return isWorkspaceRange
      ? checkInjectedWorkspacePackage(ctx, { depName, currentSpec, pkgSnapshot: pkgSnapshot! })
      : UP_TO_DATE
  }
  const isLinked = lockfileRef.startsWith('link:')
  if (isLinked) {
    if (currentSpec.startsWith('link:') || currentSpec.startsWith('file:')) {
      return linkTargetMatches(resolveSpecPath(project.dir, currentSpec.slice(5)))
    }
    if (currentSpec.startsWith('workspace:')) {
      const target = currentSpec.slice(10)
      if (isWorkspacePath(target)) {
        return linkTargetMatches(resolveSpecPath(project.dir, target))
      }
    }
  }
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
    const satisfyingWorkspacePackage = isWorkspaceRange && workspacePackagesWithName
      ? Array.from(workspacePackagesWithName.values()).find(({ manifest }) => versionSatisfies(manifest.version, availableRange))
      : undefined
    if (satisfyingWorkspacePackage != null) {
      return outdated(`Workspace package "${pkgName}" (${satisfyingWorkspacePackage.manifest.version}) satisfies range "${currentSpec}" but is not linked in the lockfile`)
    }
    return UP_TO_DATE
  }
  if (!ctx.linkWorkspacePackages && !currentSpec.startsWith('workspace:')) {
    // we found a linked dir, but we don't want to use it, because it's not specified as a
    // workspace:x.x.x dependency
    return UP_TO_DATE
  }
  if (
    isLinked &&
    isWorkspaceRange &&
    workspacePackagesWithName != null &&
    // The link may point to the package's publishConfig.directory.
    !Array.from(workspacePackagesWithName.values()).some(({ rootDir }) => isSubdirectory(path.resolve(rootDir), path.resolve(linkedDir)))
  ) {
    return outdated(`Workspace dependency "${depName}" is linked to "${linkedDir}", which is not inside a workspace package named "${pkgName}"`)
  }
  const linkedPkg = ctx.manifestsByDir[linkedDir] ?? await safeReadPackageJsonFromDir(linkedDir)
  const localPackageSatisfiesRange = versionSatisfies(linkedPkg?.version, availableRange)
  if (isLinked === localPackageSatisfiesRange) return UP_TO_DATE
  return outdated(isLinked
    ? `Linked workspace package "${depName}" (${linkedPkg?.version ?? 'unknown'}) does not satisfy range "${currentSpec}"`
    : `Workspace package "${depName}" (${linkedPkg?.version ?? 'unknown'}) satisfies range "${currentSpec}" but is not linked in the lockfile`)

  function linkTargetMatches (expectedDir: string): CheckLinkedPackagesResult {
    return expectedDir === resolveSpecPath(project.dir, lockfileRef.slice(5))
      ? UP_TO_DATE
      : outdated(`Dependency "${depName}" is linked to "${lockfileRef}", which does not match "${currentSpec}"`)
  }
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

function versionSatisfies (version: string | undefined, range: string): boolean {
  // This should pass the same options to semver as @pnpm/resolving.npm-resolver
  return range === '*' || range === '^' || range === '~' ||
    (version != null && semver.satisfies(version, range, { loose: true }))
}

function outdated (detailedReason: string): CheckLinkedPackagesResult {
  return { upToDate: false, detailedReason }
}

async function isLocalFileDepUpdated (
  lockfileDir: string,
  pkgSnapshot: PackageSnapshot | undefined,
  manifestsByDir?: Record<string, DependencyManifest>,
  workspaceDir?: string
): Promise<boolean> {
  if (!pkgSnapshot) return false
  if (!('directory' in (pkgSnapshot.resolution ?? {}))) return true
  const workspaceRoot = workspaceDir ?? lockfileDir
  const localDepDir = path.join(lockfileDir, (pkgSnapshot.resolution as DirectoryResolution).directory)
  const manifest = manifestsByDir?.[localDepDir] ?? await safeReadPackageJsonFromDir(localDepDir)
  if (!manifest) {
    // A directory without a package.json has no dependencies.
    return fs.existsSync(localDepDir) &&
      [pkgSnapshot.dependencies, pkgSnapshot.optionalDependencies, pkgSnapshot.peerDependencies]
        .every((deps) => deps == null || Object.keys(deps).length === 0)
  }
  const manifestPeerMeta = manifest.peerDependenciesMeta ?? {}
  const lockfilePeerMeta = pkgSnapshot.peerDependenciesMeta ?? {}
  for (const [name, meta] of Object.entries(manifestPeerMeta)) {
    if (Boolean(meta?.optional) !== Boolean(lockfilePeerMeta[name]?.optional)) {
      return false
    }
  }
  for (const [name, meta] of Object.entries(lockfilePeerMeta)) {
    if (Boolean(meta?.optional) !== Boolean(manifestPeerMeta[name]?.optional)) {
      return false
    }
  }
  return pEvery.default(DEPENDENCIES_OR_PEER_FIELDS, async (depField) => {
    if (depField === 'devDependencies') return true
    const manifestDeps = manifest[depField] ?? {}
    const lockfileDeps = pkgSnapshot[depField] ?? {}

    // Resolved peer dependencies are recorded next to the regular dependencies.
    if (Object.keys(lockfileDeps).some(depName => !manifestDeps[depName] && !manifest.peerDependencies?.[depName])) {
      return false
    }
    if (depField === 'peerDependencies') {
      return Object.entries(manifestDeps).every(([depName, range]) => lockfileDeps[depName] === range)
    }

    return pEvery.default(Object.keys(manifestDeps), async (depName) => {
      if (!lockfileDeps[depName]) {
        return false
      }
      const currentSpec = manifestDeps[depName]
      const lockfileDep = lockfileDeps[depName]
      if (currentSpec.startsWith('link:')) {
        return (
          lockfileDep.startsWith('link:') &&
          resolveSpecPath(localDepDir, currentSpec.slice(5)) === resolveSpecPath(lockfileDir, lockfileDep.slice(5))
        )
      }
      const cleanLockfileDep = removeSuffix(lockfileDep)
      const lockfilePath = getLocalPath(cleanLockfileDep)
      if (currentSpec.startsWith('file:')) {
        return lockfilePath != null &&
          resolveSpecPath(localDepDir, currentSpec.slice(5)) === resolveSpecPath(lockfileDir, lockfilePath)
      }
      if (currentSpec.startsWith('workspace:')) {
        const target = currentSpec.slice(10)
        if (isWorkspacePath(target)) {
          return lockfilePath != null &&
            resolveSpecPath(localDepDir, target) === resolveSpecPath(lockfileDir, lockfilePath)
        }
        const range = getVersionRange(currentSpec)
        if (lockfilePath != null) {
          if (path.isAbsolute(lockfilePath)) {
            return false
          }
          const targetDir = path.resolve(lockfileDir, lockfilePath)
          if (!isSubdirectory(workspaceRoot, targetDir)) {
            return false
          }
          let realTargetDir: string
          let realWorkspaceRoot: string
          let realManifestPath: string
          try {
            [realTargetDir, realWorkspaceRoot, realManifestPath] = await Promise.all([
              fs.promises.realpath(targetDir),
              fs.promises.realpath(workspaceRoot),
              fs.promises.realpath(path.join(targetDir, 'package.json')),
            ])
          } catch {
            return false
          }
          if (!isSubdirectory(realWorkspaceRoot, realTargetDir) || !isSubdirectory(realWorkspaceRoot, realManifestPath)) {
            return false
          }
          const targetPkg = manifestsByDir?.[targetDir] ?? await safeReadPackageJsonFromDir(targetDir)
          const expectedName = getTargetPkgName(currentSpec, depName)
          if (!targetPkg || targetPkg.name !== expectedName) {
            return false
          }
          if (range !== '*' && range !== '^' && range !== '~' && range !== '' &&
              !semver.satisfies(targetPkg.version, range, { loose: true })) {
            return false
          }
          return true
        }
        const expectedName = getTargetPkgName(currentSpec, depName)
        const actualName = getDepActualName(cleanLockfileDep, depName)
        if (actualName !== expectedName) {
          return false
        }
        const lockfileVersion = getDepVersion(cleanLockfileDep)
        if (semver.valid(lockfileVersion) && !semver.satisfies(lockfileVersion, range, { loose: true })) {
          return false
        }
        return true
      }
      const expectedName = getTargetPkgName(currentSpec, depName)
      const actualName = getDepActualName(cleanLockfileDep, depName)
      if (actualName !== expectedName) {
        return false
      }
      const lockfileVersion = getDepVersion(cleanLockfileDep)
      return semver.satisfies(lockfileVersion, getVersionRange(currentSpec), { loose: true })
    })
  })
}

function getLocalPath (lockfileDep: string): string | null {
  return lockfileDep.startsWith('link:') || lockfileDep.startsWith('file:')
    ? lockfileDep.slice(5)
    : null
}

function getDepActualName (lockfileDep: string, defaultName: string): string {
  const atIndex = lockfileDep.lastIndexOf('@')
  if (atIndex > 0) {
    return lockfileDep.slice(0, atIndex)
  }
  return defaultName
}

function getDepVersion (lockfileDep: string): string {
  const atIndex = lockfileDep.lastIndexOf('@')
  const ver = atIndex > 0 ? lockfileDep.slice(atIndex + 1) : lockfileDep
  const colonIndex = ver.indexOf(':')
  return colonIndex >= 0 ? ver.slice(colonIndex + 1) : ver
}

function isSubdirectory (parentDir: string, childPath: string): boolean {
  const relativePath = path.relative(parentDir, childPath)
  return relativePath === '' || (
    relativePath !== '..' &&
    !relativePath.startsWith(`..${path.sep}`) &&
    !path.isAbsolute(relativePath)
  )
}

function resolveSpecPath (baseDir: string, rawPath: string): string {
  const clean = rawPath.startsWith('./') ? rawPath.slice(2) : rawPath
  if (clean.startsWith('~/') || clean.startsWith('~\\')) {
    return path.resolve(os.homedir(), clean.slice(2))
  }
  return path.resolve(baseDir, clean)
}

function isWorkspacePath (spec: string): boolean {
  return (
    spec.startsWith('.') ||
    spec.startsWith('/') ||
    spec.startsWith('\\') ||
    spec.startsWith('~/') ||
    spec.startsWith('~\\') ||
    /^[a-z]:/i.test(spec)
  )
}

function getTargetPkgName (spec: string, defaultName: string): string {
  if (spec.startsWith('workspace:')) {
    const raw = spec.slice(10)
    if (isWorkspacePath(raw)) return defaultName
    const atIndex = raw.lastIndexOf('@')
    if (atIndex > 0) {
      return raw.slice(0, atIndex)
    }
  } else if (spec.startsWith('npm:')) {
    const raw = spec.slice(4)
    if (semver.validRange(raw)) {
      return defaultName
    }
    const atIndex = raw.lastIndexOf('@')
    if (atIndex > 0) {
      return raw.slice(0, atIndex)
    }
    return raw
  }
  return defaultName
}

function getVersionRange (spec: string): string {
  if (spec.startsWith('workspace:')) {
    const raw = spec.slice(10)
    const atIndex = raw.lastIndexOf('@')
    if (atIndex > 0) {
      return raw.slice(atIndex + 1) || '*'
    }
    return raw
  }
  if (spec.startsWith('npm:')) {
    const raw = spec.slice(4)
    if (semver.validRange(raw)) {
      return raw
    }
    const index = raw.indexOf('@', 1)
    if (index === -1) return '*'
    return raw.slice(index + 1) || '*'
  }
  return spec
}
