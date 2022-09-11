import resolveDependencies, { getWantedDependencies } from '@pnpm/resolve-dependencies'
import { PeerDependencyIssuesByProjects } from '@pnpm/types'
import getContext, { GetContextOptions, ProjectOptions } from '@pnpm/get-context'
import { createReadPackageHook } from '@pnpm/hooks.read-package-hook'
import { getPreferredVersionsFromLockfile } from './install/getPreferredVersions'
import { InstallOptions } from './install/extendInstallOptions'
import { DEFAULT_REGISTRIES } from '@pnpm/normalize-registries'

export type ListMissingPeersOptions = Partial<GetContextOptions>
& Pick<InstallOptions, 'hooks'
| 'ignoreCompatibilityDb'
| 'linkWorkspacePackagesDepth'
| 'nodeVersion'
| 'nodeLinker'
| 'overrides'
| 'packageExtensions'
| 'preferWorkspacePackages'
| 'saveWorkspaceProtocol'
| 'storeController'
| 'useGitBranchLockfile'
| 'workspacePackages'
>
& Pick<GetContextOptions, 'storeDir'>

export async function getPeerDependencyIssues (
  projects: ProjectOptions[],
  opts: ListMissingPeersOptions
): Promise<PeerDependencyIssuesByProjects> {
  const lockfileDir = opts.lockfileDir ?? process.cwd()
  const ctx = await getContext({
    force: false,
    forceSharedLockfile: false,
    extraBinPaths: [],
    lockfileDir,
    nodeLinker: opts.nodeLinker ?? 'isolated',
    registries: DEFAULT_REGISTRIES,
    useLockfile: true,
    allProjects: projects,
    ...opts,
  })
  const projectsToResolve = Object.values(ctx.projects).map((project) => ({
    ...project,
    updatePackageManifest: false,
    wantedDependencies: getWantedDependencies(project.manifest),
  }))
  const preferredVersions = ctx.wantedLockfile.packages ? getPreferredVersionsFromLockfile(ctx.wantedLockfile.packages) : undefined
  const {
    peerDependencyIssuesByProjects,
    waitTillAllFetchingsFinish,
  } = await resolveDependencies(
    projectsToResolve,
    {
      currentLockfile: ctx.currentLockfile,
      allowedDeprecatedVersions: {},
      allowNonAppliedPatches: false,
      defaultUpdateDepth: -1,
      dryRun: true,
      engineStrict: false,
      force: false,
      forceFullResolution: true,
      hooks: {
        readPackage: createReadPackageHook({
          ignoreCompatibilityDb: opts.ignoreCompatibilityDb,
          lockfileDir,
          overrides: opts.overrides,
          packageExtensions: opts.packageExtensions,
          readPackageHook: opts.hooks?.readPackage,
        }),
      },
      linkWorkspacePackagesDepth: opts.linkWorkspacePackagesDepth ?? (opts.saveWorkspaceProtocol ? 0 : -1),
      lockfileDir,
      nodeVersion: opts.nodeVersion ?? process.version,
      pnpmVersion: '',
      preferWorkspacePackages: opts.preferWorkspacePackages,
      preferredVersions,
      preserveWorkspaceProtocol: false,
      registries: ctx.registries,
      saveWorkspaceProtocol: false, // this doesn't matter in our case. We won't write changes to package.json files
      storeController: opts.storeController,
      tag: 'latest',
      virtualStoreDir: ctx.virtualStoreDir,
      wantedLockfile: ctx.wantedLockfile,
      workspacePackages: opts.workspacePackages ?? {},
    }
  )

  await waitTillAllFetchingsFinish()

  return peerDependencyIssuesByProjects
}
