import { type Catalogs } from '@pnpm/catalogs.types'
import { type ProjectOptions } from '@pnpm/get-context'
import {
  type LockfileObject,
  type ProjectSnapshot,
} from '@pnpm/lockfile.types'
import { refIsLocalTarball } from '@pnpm/lockfile.utils'
import { type WorkspacePackages } from '@pnpm/resolver-base'
import { DEPENDENCIES_FIELDS, type ProjectId } from '@pnpm/types'
import pEvery from 'p-every'
import any from 'ramda/src/any'
import isEmpty from 'ramda/src/isEmpty'
import { allCatalogsAreUpToDate } from './allCatalogsAreUpToDate'
import { getWorkspacePackagesByDirectory } from './getWorkspacePackagesByDirectory'
import { linkedPackagesAreUpToDate } from './linkedPackagesAreUpToDate'
import { satisfiesPackageManifest } from './satisfiesPackageManifest'

export async function allProjectsAreUpToDate (
  projects: Array<Pick<ProjectOptions, 'manifest' | 'rootDir'> & { id: ProjectId }>,
  opts: {
    catalogs: Catalogs
    autoInstallPeers: boolean
    excludeLinksFromLockfile: boolean
    linkWorkspacePackages: boolean
    wantedLockfile: LockfileObject
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
    if (importer == null) {
      return DEPENDENCIES_FIELDS.every((depType) => project.manifest[depType] == null || isEmpty(project.manifest[depType]))
    }
    return importer != null &&
      !hasLocalTarballDepsInRoot(importer) &&
      _satisfiesPackageManifest(importer, project.manifest).satisfies &&
      _linkedPackagesAreUpToDate({
        dir: project.rootDir,
        manifest: project.manifest,
        snapshot: importer,
      })
  })
}

function hasLocalTarballDepsInRoot (importer: ProjectSnapshot): boolean {
  return any(refIsLocalTarball, Object.values(importer.dependencies ?? {})) ||
    any(refIsLocalTarball, Object.values(importer.devDependencies ?? {})) ||
    any(refIsLocalTarball, Object.values(importer.optionalDependencies ?? {}))
}
