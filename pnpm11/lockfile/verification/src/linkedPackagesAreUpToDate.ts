import fs from 'node:fs'
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

export async function linkedPackagesAreUpToDate (
  {
    linkWorkspacePackages,
    manifestsByDir,
    workspacePackages,
    lockfilePackages,
    lockfileDir,
    workspaceDir,
  }: {
    linkWorkspacePackages: boolean
    manifestsByDir: Record<string, DependencyManifest>
    workspacePackages?: WorkspacePackages
    lockfilePackages?: PackageSnapshots
    lockfileDir: string
    workspaceDir?: string
  },
  project: {
    dir: string
    manifest: ProjectManifest
    snapshot: ProjectSnapshot
  }
): Promise<boolean> {
  return pEvery.default(
    DEPENDENCIES_FIELDS,
    (depField) => {
      const lockfileDeps = project.snapshot[depField]
      const manifestDeps = project.manifest[depField]
      if ((lockfileDeps == null) || (manifestDeps == null)) return true
      const depNames = Object.keys(lockfileDeps)
      return pEvery.default(
        depNames,
        async (depName) => {
          const currentSpec = manifestDeps[depName]
          if (!currentSpec) return true
          const lockfileRef = lockfileDeps[depName]
          if (refIsLocalDirectory(project.snapshot.specifiers[depName]) || refIsLocalDirectory(lockfileRef)) {
            // When a file: specifier resolves to link: in the lockfile
            // (e.g. injected self-references), it's a local link with no
            // entry in the packages section. Treat it as up-to-date.
            if (lockfileRef.startsWith('link:')) return true
            const depPath = refToRelative(lockfileRef, depName)
            return depPath != null && isLocalFileDepUpdated(lockfileDir, lockfilePackages?.[depPath], manifestsByDir, workspaceDir)
          }
          const isLinked = lockfileRef.startsWith('link:')
          if (
            isLinked &&
            (
              currentSpec.startsWith('link:') ||
              currentSpec.startsWith('file:') ||
              currentSpec.startsWith('workspace:.')
            )
          ) {
            return true
          }
          // https://github.com/pnpm/pnpm/issues/6592
          // if the dependency is linked and the specified version type is tag, we consider it to be up-to-date to skip full resolution.
          if (isLinked && getVersionSelectorType(currentSpec)?.type === 'tag') {
            return true
          }
          const linkedDir = isLinked
            ? path.join(project.dir, lockfileRef.slice(5))
            : workspacePackages?.get(depName)?.get(lockfileRef)?.rootDir
          if (!linkedDir) return true
          if (!linkWorkspacePackages && !currentSpec.startsWith('workspace:')) {
            // we found a linked dir, but we don't want to use it, because it's not specified as a
            // workspace:x.x.x dependency
            return true
          }
          const linkedPkg = manifestsByDir[linkedDir] ?? await safeReadPackageJsonFromDir(linkedDir)
          const availableRange = getVersionRange(currentSpec)
          // This should pass the same options to semver as @pnpm/resolving.npm-resolver
          const localPackageSatisfiesRange = availableRange === '*' || availableRange === '^' || availableRange === '~' ||
            linkedPkg && semver.satisfies(linkedPkg.version, availableRange, { loose: true })
          if (isLinked !== localPackageSatisfiesRange) return false
          return true
        }
      )
    }
  )
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
  if (!manifest) return false
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

    if (Object.keys(lockfileDeps).some(depName => !manifestDeps[depName])) {
      return false
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
          path.resolve(localDepDir, currentSpec.slice(5)) === path.resolve(lockfileDir, lockfileDep.slice(5))
        )
      }
      if (currentSpec.startsWith('file:')) {
        const cleanLockfileDep = removeSuffix(lockfileDep)
        const lockfilePath = cleanLockfileDep.startsWith('link:') || cleanLockfileDep.startsWith('file:')
          ? cleanLockfileDep.slice(5)
          : null
        if (lockfilePath == null) return false
        return path.resolve(localDepDir, currentSpec.slice(5)) === path.resolve(lockfileDir, lockfilePath)
      }
      if (currentSpec.startsWith('workspace:')) {
        const target = currentSpec.slice(10)
        if (isWorkspacePath(target)) {
          const cleanLockfileDep = removeSuffix(lockfileDep)
          const lockfilePath = cleanLockfileDep.startsWith('link:') || cleanLockfileDep.startsWith('file:')
            ? cleanLockfileDep.slice(5)
            : null
          if (lockfilePath == null) return false
          const cleanTarget = target.startsWith('./') ? target.slice(2) : target
          return path.resolve(localDepDir, cleanTarget) === path.resolve(lockfileDir, lockfilePath)
        }
        const range = getVersionRange(currentSpec)
        const cleanLockfileDep = removeSuffix(lockfileDep)
        if (cleanLockfileDep.startsWith('link:') || cleanLockfileDep.startsWith('file:')) {
          const depPath = cleanLockfileDep.slice(5)
          if (path.isAbsolute(depPath)) {
            return false
          }
          const targetDir = cleanLockfileDep.startsWith('link:')
            ? path.resolve(lockfileDir, depPath)
            : path.resolve(localDepDir, depPath)
          const relToWorkspace = path.relative(workspaceRoot, targetDir)
          if (relToWorkspace.startsWith('..') || path.isAbsolute(relToWorkspace)) {
            return false
          }
          let realTargetDir: string
          let realWorkspaceRoot: string
          try {
            realTargetDir = fs.realpathSync(targetDir)
            realWorkspaceRoot = fs.realpathSync(workspaceRoot)
          } catch {
            return false
          }
          const relReal = path.relative(realWorkspaceRoot, realTargetDir)
          if (relReal.startsWith('..') || path.isAbsolute(relReal)) {
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
      const cleanLockfileDep = removeSuffix(lockfileDeps[depName])
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

function isWorkspacePath (spec: string): boolean {
  return spec.startsWith('.') || spec.startsWith('/') || spec.startsWith('~/') || /^[a-z]:/i.test(spec)
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
