import {
  resolveDependencies,
  getWantedDependencies,
} from '@pnpm/resolve-dependencies'
import { getContext } from '@pnpm/get-context'
import { DEFAULT_REGISTRIES } from '@pnpm/normalize-registries'
import type { ProjectOptions } from '@pnpm/read-projects-context'
import { createReadPackageHook } from '@pnpm/hooks.read-package-hook'
import type { PeerDependencyIssuesByProjects, GetContextOptions, InstallOptions } from '@pnpm/types'

import { getPreferredVersionsFromLockfileAndManifests } from './install/getPreferredVersions'

export type ListMissingPeersOptions = Partial<GetContextOptions> &
  Pick<
    InstallOptions,
    | 'hooks'
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
  > &
  Partial<Pick<InstallOptions, 'supportedArchitectures'>> &
  Pick<
    GetContextOptions,
    'autoInstallPeers' | 'excludeLinksFromLockfile' | 'storeDir'
  >

export async function getPeerDependencyIssues(
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
    // @ts-ignore
    allProjects: projects,
    ...opts,
  })

  const projectsToResolve = Object.values(ctx.projects).map((project) => ({
    ...project,
    updatePackageManifest: false,
    wantedDependencies: getWantedDependencies(project.manifest),
  }))

  const preferredVersions = getPreferredVersionsFromLockfileAndManifests(
    ctx.wantedLockfile.packages,
    Object.values(ctx.projects).map(({ manifest }) => manifest).filter(Boolean)
  )

  const { peerDependencyIssuesByProjects, waitTillAllFetchingsFinish } =
    await resolveDependencies(projectsToResolve, {
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
      linkWorkspacePackagesDepth:
        opts.linkWorkspacePackagesDepth ??
        (opts.saveWorkspaceProtocol ? 0 : -1),
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
      supportedArchitectures: opts.supportedArchitectures,
    })

  await waitTillAllFetchingsFinish()

  return peerDependencyIssuesByProjects
}
