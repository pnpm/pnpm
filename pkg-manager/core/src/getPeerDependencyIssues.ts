import { getPreferredVersionsFromLockfileAndManifests } from '@pnpm/lockfile.preferred-versions'
import { resolveDependencies, getWantedDependencies } from '@pnpm/resolve-dependencies'
import { type PeerDependencyIssuesByProjects } from '@pnpm/types'
import { getContext, type GetContextOptions, type ProjectOptions } from '@pnpm/get-context'
import { createReadPackageHook } from '@pnpm/hooks.read-package-hook'
import { type InstallOptions } from './install/extendInstallOptions'
import { DEFAULT_REGISTRIES } from '@pnpm/normalize-registries'

export type ListMissingPeersOptions = Partial<GetContextOptions>
& Pick<InstallOptions, 'hooks'
| 'catalogs'
| 'dedupePeerDependents'
| 'ignoreCompatibilityDb'
| 'linkWorkspacePackagesDepth'
| 'nodeVersion'
| 'nodeLinker'
| 'overrides'
| 'packageExtensions'
| 'ignoredOptionalDependencies'
| 'preferWorkspacePackages'
| 'saveWorkspaceProtocol'
| 'storeController'
| 'useGitBranchLockfile'
| 'peersSuffixMaxLength'
>
& Partial<Pick<InstallOptions, 'supportedArchitectures'>>
& Pick<GetContextOptions, 'autoInstallPeers' | 'excludeLinksFromLockfile' | 'storeDir'>
& Required<Pick<InstallOptions, 'virtualStoreDirMaxLength' | 'peersSuffixMaxLength'>>

export async function getPeerDependencyIssues (
  projects: ProjectOptions[],
  opts: ListMissingPeersOptions
): Promise<PeerDependencyIssuesByProjects> {
  const lockfileDir = opts.lockfileDir ?? process.cwd()
  const ctx = await getContext({
    force: false,
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
  const preferredVersions = getPreferredVersionsFromLockfileAndManifests(
    ctx.wantedLockfile.packages,
    Object.values(ctx.projects).map(({ manifest }) => manifest)
  )
  const {
    peerDependencyIssuesByProjects,
    waitTillAllFetchingsFinish,
  } = await resolveDependencies(
    projectsToResolve,
    {
      currentLockfile: ctx.currentLockfile,
      allowedDeprecatedVersions: {},
      allowNonAppliedPatches: false,
      catalogs: opts.catalogs,
      defaultUpdateDepth: -1,
      dedupePeerDependents: opts.dedupePeerDependents,
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
          ignoredOptionalDependencies: opts.ignoredOptionalDependencies,
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
      virtualStoreDirMaxLength: ctx.virtualStoreDirMaxLength,
      wantedLockfile: ctx.wantedLockfile,
      workspacePackages: ctx.workspacePackages ?? new Map(),
      supportedArchitectures: opts.supportedArchitectures,
      peersSuffixMaxLength: opts.peersSuffixMaxLength,
    }
  )

  await waitTillAllFetchingsFinish()

  return peerDependencyIssuesByProjects
}
