import type { Catalogs } from '@pnpm/catalogs.types'
import type { ProjectOptions } from '@pnpm/installing.context'
import type {
  LockfileObject,
} from '@pnpm/lockfile.types'
import type { WorkspacePackages } from '@pnpm/resolving.resolver-base'
import { DEPENDENCIES_FIELDS, type DependencyManifest, type ProjectId, type ReadPackageHook } from '@pnpm/types'
import pEvery from 'p-every'
import { isEmpty } from 'ramda'

import { allCatalogsAreUpToDate } from './allCatalogsAreUpToDate.js'
import { catalogResolutionsAreUpToDate } from './catalogResolutionsAreUpToDate.js'
import { getWorkspacePackagesByDirectory } from './getWorkspacePackagesByDirectory.js'
import { linkedPackagesAreUpToDate, type LinkedPackagesContext } from './linkedPackagesAreUpToDate.js'
import { localTarballDepsAreUpToDate } from './localTarballDepsAreUpToDate.js'
import { satisfiesPackageManifest } from './satisfiesPackageManifest.js'

export interface AllProjectsAreUpToDateOptions {
  catalogs: Catalogs
  autoInstallPeers: boolean
  excludeLinksFromLockfile: boolean
  ignoredOptionalDependencies?: string[]
  linkWorkspacePackages: boolean
  wantedLockfile: LockfileObject
  workspacePackages: WorkspacePackages
  lockfileDir: string
  workspaceDir?: string
  readPackageHook?: ReadPackageHook
}

export async function allProjectsAreUpToDate (
  projects: Array<Pick<ProjectOptions, 'manifest' | 'rootDir'> & { id: ProjectId }>,
  opts: AllProjectsAreUpToDateOptions
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
    ignoredOptionalDependencies: opts.ignoredOptionalDependencies,
  })
  const _linkedPackagesAreUpToDate = linkedPackagesAreUpToDate.bind(null, createLinkedPackagesContext(opts, manifestsByDir))
  const _localTarballDepsAreUpToDate = localTarballDepsAreUpToDate.bind(null, {
    fileIntegrityCache: new Map(),
    lockfilePackages: opts.wantedLockfile.packages,
    lockfileDir: opts.lockfileDir,
  })
  return pEvery.default(projects, async (project) => {
    const importer = opts.wantedLockfile.importers[project.id]
    if (importer == null) {
      return DEPENDENCIES_FIELDS.every((depType) => project.manifest[depType] == null || isEmpty(project.manifest[depType]))
    }

    const projectInfo = {
      dir: project.rootDir,
      manifest: project.manifest,
      snapshot: importer,
    }

    return importer != null &&
      _satisfiesPackageManifest(importer, project.manifest).satisfies &&
      catalogResolutionsAreUpToDate(importer, opts.wantedLockfile.catalogs) &&
      (await _localTarballDepsAreUpToDate(projectInfo)) &&
      _linkedPackagesAreUpToDate(projectInfo)
  })
}

function createLinkedPackagesContext (
  opts: AllProjectsAreUpToDateOptions,
  manifestsByDir: Record<string, DependencyManifest>
): LinkedPackagesContext {
  return {
    linkWorkspacePackages: opts.linkWorkspacePackages,
    manifestsByDir,
    workspacePackages: opts.workspacePackages,
    lockfilePackages: opts.wantedLockfile.packages,
    lockfileDir: opts.lockfileDir,
    workspaceDir: opts.workspaceDir,
    injectWorkspacePackages: opts.wantedLockfile.settings?.injectWorkspacePackages,
    resolution: {
      autoInstallPeers: opts.autoInstallPeers,
      catalogs: opts.catalogs,
      readPackageHook: opts.readPackageHook,
    },
    hookedManifests: new Map(),
  }
}
