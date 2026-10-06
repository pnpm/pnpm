import type { RangeSpecStyle } from '@pnpm/installing.deps-resolver'
import type { DependenciesField, ProjectManifest, ProjectRootDir } from '@pnpm/types'

import type { BeforeLifecycleScriptsResult, InstallOptions } from './extendInstallOptions.js'
import { rootProjectRunsPreinstallEarly } from './installPredicates.js'
import { installViaPnprServer } from './installViaPnprServer.js'
import { mutateModules } from './mutateModules.js'
import type {
  InstallMutationOptions,
  InstallResult,
  MutatedProject,
  MutateModulesInSingleProjectResult,
  MutateModulesOptions,
  SingleProjectInstallOptions,
} from './mutationTypes.js'
import { canUsePnprForInstall, pnpmfileHookPnprCannotRun } from './pnpr.js'

export type { BeforeLifecycleScriptsResult }
export { IgnoredBuildsError } from './ignoredBuilds.js'
export { mutateModules } from './mutateModules.js'
export type {
  DependenciesMutation,
  DryRunInstallResult,
  ImporterToUpdate,
  InstallDepsMutation,
  InstallResult,
  InstallSomeDepsMutation,
  MutatedProject,
  MutateModulesInSingleProjectResult,
  MutateModulesOptions,
  MutateModulesResult,
  UninstallSomeDepsMutation,
  UpdatedProject,
} from './mutationTypes.js'

export async function install (
  manifest: ProjectManifest,
  opts: SingleProjectInstallOptions
): Promise<InstallResult> {
  const rootDir = (opts.dir ?? process.cwd()) as ProjectRootDir

  // When a pnpr server is configured, use server-side resolution
  // instead of the normal resolution flow.
  if (opts.pnprServer && canUsePnprForInstall(opts) && pnpmfileHookPnprCannotRun(opts.hooks) == null) {
    return installViaPnprServer({
      manifest,
      rootDir,
      opts,
      rootProjectPreinstallRan: rootProjectRunsPreinstallEarly(
        [{ rootDir, mutation: 'install' }],
        { ...opts, lockfileDir: opts.lockfileDir ?? rootDir }
      ),
    })
  }

  const { updatedCatalogs, updatedProjects: projects, ignoredBuilds, newLockfile, resolutionPolicyViolations, dryRunResult } = await mutateModules(
    [
      {
        mutation: 'install',
        pruneDirectDependencies: opts.pruneDirectDependencies,
        rootDir,
        update: opts.update,
        updatePatches: opts.updatePatches,
        updateMatching: opts.updateMatching,
        updateToLatest: opts.updateToLatest,
        updatePackageManifest: opts.updatePackageManifest,
      },
    ],
    {
      ...opts,
      allProjects: [{
        buildIndex: 0,
        manifest,
        rootDir,
        binsDir: opts.binsDir,
      }],
    }
  )
  return { updatedCatalogs, updatedManifest: projects[0].manifest, ignoredBuilds, newLockfile, resolutionPolicyViolations, dryRunResult }
}

export async function mutateModulesInSingleProject (
  project: MutatedProject & {
    binsDir?: string
    manifest: ProjectManifest
    rootDir: ProjectRootDir
    modulesDir?: string
  },
  maybeOpts: Omit<MutateModulesOptions, 'allProjects'> & InstallMutationOptions
): Promise<MutateModulesInSingleProjectResult> {
  const result = await mutateModules(
    [
      {
        ...project,
        update: maybeOpts.update,
        updatePatches: maybeOpts.updatePatches,
        updateToLatest: maybeOpts.updateToLatest,
        updateMatching: maybeOpts.updateMatching,
        updatePackageManifest: maybeOpts.updatePackageManifest,
      } as MutatedProject,
    ],
    {
      ...maybeOpts,
      allProjects: [{
        buildIndex: 0,
        ...project,
      }],
    }
  )
  return {
    updatedCatalogs: result.updatedCatalogs,
    updatedProject: result.updatedProjects[0],
    ignoredBuilds: result.ignoredBuilds,
    newLockfile: result.newLockfile,
    wantedLockfile: result.wantedLockfile,
    resolutionPolicyViolations: result.resolutionPolicyViolations,
    dryRunResult: result.dryRunResult,
  }
}

export async function addDependenciesToPackage (
  manifest: ProjectManifest,
  dependencySelectors: string[],
  opts: Omit<InstallOptions, 'allProjects'> & {
    bin?: string
    allowNew?: boolean
    peer?: boolean
    savePeer?: boolean
    rangeSpecStyle?: RangeSpecStyle
    targetDependenciesField?: DependenciesField
  } & InstallMutationOptions
): Promise<InstallResult> {
  const rootDir = (opts.dir ?? process.cwd()) as ProjectRootDir
  const { updatedCatalogs, updatedProjects: projects, ignoredBuilds, newLockfile, resolutionPolicyViolations } = await mutateModules(
    [
      {
        allowNew: opts.allowNew,
        dependencySelectors,
        mutation: 'installSome',
        peer: opts.peer,
        savePeer: opts.savePeer,
        rangeSpecStyle: opts.rangeSpecStyle,
        rootDir,
        targetDependenciesField: opts.targetDependenciesField,
        update: opts.update,
        updatePatches: opts.updatePatches,
        updateMatching: opts.updateMatching,
        updatePackageManifest: opts.updatePackageManifest,
        updateToLatest: opts.updateToLatest,
      },
    ],
    {
      ...opts,
      lockfileDir: opts.lockfileDir ?? opts.dir,
      allProjects: [
        {
          buildIndex: 0,
          binsDir: opts.bin,
          manifest,
          rootDir,
        },
      ],
    })
  return { updatedCatalogs, updatedManifest: projects[0].manifest, ignoredBuilds, newLockfile, resolutionPolicyViolations }
}
