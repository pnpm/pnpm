import path from 'path'
import { type Catalogs } from '@pnpm/catalogs.types'
import { type ProjectOptions } from '@pnpm/get-context'
import {
  type PackageSnapshot,
  type Lockfile,
  type ProjectSnapshot,
  type PackageSnapshots,
} from '@pnpm/lockfile-file'
import { refIsLocalDirectory, refIsLocalTarball, satisfiesPackageManifest } from '@pnpm/lockfile-utils'
import { safeReadPackageJsonFromDir } from '@pnpm/read-package-json'
import { refToRelative } from '@pnpm/dependency-path'
import { type DirectoryResolution, type WorkspacePackages } from '@pnpm/resolver-base'
import {
  DEPENDENCIES_FIELDS,
  DEPENDENCIES_OR_PEER_FIELDS,
  type DependencyManifest,
  type ProjectId,
  type ProjectManifest,
} from '@pnpm/types'
import pEvery from 'p-every'
import any from 'ramda/src/any'
import semver from 'semver'
import getVersionSelectorType from 'version-selector-type'
import { allCatalogsAreUpToDate } from './allCatalogsAreUpToDate'

export async function allProjectsAreUpToDate (
  projects: Array<Pick<ProjectOptions, 'manifest' | 'rootDir'> & { id: ProjectId }>,
  opts: {
    catalogs: Catalogs
    autoInstallPeers: boolean
    excludeLinksFromLockfile: boolean
    linkWorkspacePackages: boolean
    wantedLockfile: Lockfile
    workspacePackages: WorkspacePackages
    lockfileDir: string
  }
): Promise<boolean> {
  // Projects may declare dependencies using catalog protocol specifiers. If the
  // catalog config definitions are edited by users, projects using them are out
  // of date.
  if (!allCatalogsAreUpToDate(opts.catalogs, opts.wantedLockfile.catalogs)) {
    return false
  }

  const manifestsByDir = opts.workspacePackages ? getWorkspacePackagesByDirectory(opts.workspacePackages) : {}
  const _satisfiesPackageManifest = satisfiesPackageManifest.bind(null, {
    autoInstallPeers: opts.autoInstallPeers,
    excludeLinksFromLockfile: opts.excludeLinksFromLockfile,
  })
  const _linkedPackagesAreUpToDate = linkedPackagesAreUpToDate.bind(null, {
    linkWorkspacePackages: opts.linkWorkspacePackages,
    manifestsByDir,
    workspacePackages: opts.workspacePackages,
    lockfilePackages: opts.wantedLockfile.packages,
    lockfileDir: opts.lockfileDir,
  })
  return pEvery(projects, (project) => {
    const importer = opts.wantedLockfile.importers[project.id]
    return !hasLocalTarballDepsInRoot(importer) &&
      _satisfiesPackageManifest(importer, project.manifest).satisfies &&
      _linkedPackagesAreUpToDate({
        dir: project.rootDir,
        manifest: project.manifest,
        snapshot: importer,
      })
  })
}

function getWorkspacePackagesByDirectory (workspacePackages: WorkspacePackages): Record<string, DependencyManifest> {
  const workspacePackagesByDirectory: Record<string, DependencyManifest> = {}
  if (workspacePackages) {
    for (const pkgVersions of workspacePackages.values()) {
      for (const { rootDir, manifest } of pkgVersions.values()) {
        workspacePackagesByDirectory[rootDir] = manifest
      }
    }
  }
  return workspacePackagesByDirectory
}

async function linkedPackagesAreUpToDate (
  {
    linkWorkspacePackages,
    manifestsByDir,
    workspacePackages,
    lockfilePackages,
    lockfileDir,
  }: {
    linkWorkspacePackages: boolean
    manifestsByDir: Record<string, DependencyManifest>
    workspacePackages: WorkspacePackages
    lockfilePackages?: PackageSnapshots
    lockfileDir: string
  },
  project: {
    dir: string
    manifest: ProjectManifest
    snapshot: ProjectSnapshot
  }
): Promise<boolean> {
  return pEvery(
    DEPENDENCIES_FIELDS,
    (depField) => {
      const lockfileDeps = project.snapshot[depField]
      const manifestDeps = project.manifest[depField]
      if ((lockfileDeps == null) || (manifestDeps == null)) return true
      const depNames = Object.keys(lockfileDeps)
      return pEvery(
        depNames,
        async (depName) => {
          const currentSpec = manifestDeps[depName]
          if (!currentSpec) return true
          const lockfileRef = lockfileDeps[depName]
          if (refIsLocalDirectory(project.snapshot.specifiers[depName])) {
            const depPath = refToRelative(lockfileRef, depName)
            return depPath != null && isLocalFileDepUpdated(lockfileDir, lockfilePackages?.[depPath])
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
          // This should pass the same options to semver as @pnpm/npm-resolver
          const localPackageSatisfiesRange = availableRange === '*' || availableRange === '^' || availableRange === '~' ||
            linkedPkg && semver.satisfies(linkedPkg.version, availableRange, { loose: true })
          if (isLinked !== localPackageSatisfiesRange) return false
          return true
        }
      )
    }
  )
}

async function isLocalFileDepUpdated (lockfileDir: string, pkgSnapshot: PackageSnapshot | undefined): Promise<boolean> {
  if (!pkgSnapshot) return false
  const localDepDir = path.join(lockfileDir, (pkgSnapshot.resolution as DirectoryResolution).directory)
  const manifest = await safeReadPackageJsonFromDir(localDepDir)
  if (!manifest) return false
  for (const depField of DEPENDENCIES_OR_PEER_FIELDS) {
    if (depField === 'devDependencies') continue
    const manifestDeps = manifest[depField] ?? {}
    const lockfileDeps = pkgSnapshot[depField] ?? {}

    // Lock file has more dependencies than the current manifest, e.g. some dependencies are removed.
    if (Object.keys(lockfileDeps).some(depName => !manifestDeps[depName])) {
      return false
    }

    for (const depName of Object.keys(manifestDeps)) {
      // If a dependency does not exist in the lock file, e.g. a new dependency is added to the current manifest.
      // We need to do full resolution again.
      if (!lockfileDeps[depName]) {
        return false
      }
      const currentSpec = manifestDeps[depName]
      // We do not care about the link dependencies of local dependency.
      if (currentSpec.startsWith('file:') || currentSpec.startsWith('link:') || currentSpec.startsWith('workspace:')) continue
      if (semver.satisfies(lockfileDeps[depName], getVersionRange(currentSpec), { loose: true })) {
        continue
      } else {
        return false
      }
    }
  }
  return true
}

function getVersionRange (spec: string): string {
  if (spec.startsWith('workspace:')) return spec.slice(10)
  if (spec.startsWith('npm:')) {
    spec = spec.slice(4)
    const index = spec.indexOf('@', 1)
    if (index === -1) return '*'
    return spec.slice(index + 1) || '*'
  }
  return spec
}

function hasLocalTarballDepsInRoot (importer: ProjectSnapshot): boolean {
  return any(refIsLocalTarball, Object.values(importer.dependencies ?? {})) ||
    any(refIsLocalTarball, Object.values(importer.devDependencies ?? {})) ||
    any(refIsLocalTarball, Object.values(importer.optionalDependencies ?? {}))
}
