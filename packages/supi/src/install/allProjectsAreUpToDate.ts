import { ProjectOptions } from '@pnpm/get-context'
import {
  Lockfile,
  ProjectSnapshot,
} from '@pnpm/lockfile-file'
import { satisfiesPackageManifest } from '@pnpm/lockfile-utils'
import { safeReadPackageFromDir as safeReadPkgFromDir } from '@pnpm/read-package-json'
import { WorkspacePackages } from '@pnpm/resolver-base'
import {
  DEPENDENCIES_FIELDS,
  DependencyManifest,
  ProjectManifest,
} from '@pnpm/types'
import pEvery from 'p-every'
import path = require('path')
import R = require('ramda')
import semver = require('semver')

export default async function allProjectsAreUpToDate (
  projects: Array<ProjectOptions & { id: string }>,
  opts: {
    wantedLockfile: Lockfile,
    workspacePackages: WorkspacePackages,
  }
) {
  const manifestsByDir = opts.workspacePackages ? getWorkspacePackagesByDirectory(opts.workspacePackages) : {}
  const _satisfiesPackageManifest = satisfiesPackageManifest.bind(null, opts.wantedLockfile)
  const _linkedPackagesAreUpToDate = linkedPackagesAreUpToDate.bind(null, manifestsByDir, opts.workspacePackages)
  return pEvery(projects, async (project) => {
    const importer = opts.wantedLockfile.importers[project.id]
    return importer && !hasLocalTarballDepsInRoot(importer) &&
      _satisfiesPackageManifest(project.manifest, project.id) &&
      _linkedPackagesAreUpToDate(project.manifest, importer, project.rootDir)
  })
}

function getWorkspacePackagesByDirectory (workspacePackages: WorkspacePackages) {
  const workspacePackagesByDirectory = {}
  Object.keys(workspacePackages || {}).forEach((pkgName) => {
    Object.keys(workspacePackages[pkgName] || {}).forEach((pkgVersion) => {
      workspacePackagesByDirectory[workspacePackages[pkgName][pkgVersion].dir] = workspacePackages[pkgName][pkgVersion].manifest
    })
  })
  return workspacePackagesByDirectory
}

async function linkedPackagesAreUpToDate (
  manifestsByDir: Record<string, DependencyManifest>,
  workspacePackages: WorkspacePackages,
  manifest: ProjectManifest,
  projectSnapshot: ProjectSnapshot,
  projectDir: string
) {
  for (const depField of DEPENDENCIES_FIELDS) {
    const lockfileDeps = projectSnapshot[depField]
    const manifestDeps = manifest[depField]
    if (!lockfileDeps || !manifestDeps) continue
    const depNames = Object.keys(lockfileDeps)
    for (const depName of depNames) {
      const currentSpec = manifestDeps[depName]
      if (!currentSpec) continue
      const lockfileRef = lockfileDeps[depName]
      const isLinked = lockfileRef.startsWith('link:')
      if (
        isLinked &&
        (
          currentSpec.startsWith('link:') ||
          currentSpec.startsWith('file:')
        )
      ) {
        continue
      }
      const linkedDir = isLinked
        ? path.join(projectDir, lockfileRef.substr(5))
        : workspacePackages?.[depName]?.[lockfileRef]?.dir
      if (!linkedDir) continue
      const linkedPkg = manifestsByDir[linkedDir] ?? await safeReadPkgFromDir(linkedDir)
      const availableRange = getVersionRange(currentSpec)
      // This should pass the same options to semver as @pnpm/npm-resolver
      const localPackageSatisfiesRange = availableRange === '*' ||
        linkedPkg && semver.satisfies(linkedPkg.version, availableRange, { loose: true })
      if (isLinked !== localPackageSatisfiesRange) return false
    }
  }
  return true
}

function getVersionRange (spec: string) {
  if (spec.startsWith('workspace:')) return spec.substr(10)
  if (spec.startsWith('npm:')) {
    spec = spec.substr(4)
    const index = spec.indexOf('@', 1)
    if (index === -1) return '*'
    return spec.substr(index + 1) || '*'
  }
  return spec
}

function hasLocalTarballDepsInRoot (importer: ProjectSnapshot) {
  return R.any(refIsLocalTarball, Object.values(importer.dependencies ?? {}))
    || R.any(refIsLocalTarball, Object.values(importer.devDependencies ?? {}))
    || R.any(refIsLocalTarball, Object.values(importer.optionalDependencies ?? {}))
}

function refIsLocalTarball (ref: string) {
  return ref.startsWith('file:') && (ref.endsWith('.tgz') || ref.endsWith('.tar.gz') || ref.endsWith('.tar'))
}
