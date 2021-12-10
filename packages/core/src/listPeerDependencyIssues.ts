import resolveDependencies from '@pnpm/resolve-dependencies'
import getWantedDependencies from '@pnpm/resolve-dependencies/lib/getWantedDependencies'
import { PeerDependencyIssues } from '@pnpm/types'
import getContext, { GetContextOptions, ProjectOptions } from '@pnpm/get-context'
import { createReadPackageHook } from './install'
import { getPreferredVersionsFromLockfile } from './install/getPreferredVersions'
import { InstallOptions } from './install/extendInstallOptions'
import { DEFAULT_REGISTRIES } from '@pnpm/normalize-registries'
import { intersect } from 'semver-intersect'

export type ListMissingPeersOptions = Partial<GetContextOptions>
& Pick<InstallOptions, 'hooks'
| 'linkWorkspacePackagesDepth'
| 'nodeVersion'
| 'overrides'
| 'packageExtensions'
| 'preferWorkspacePackages'
| 'saveWorkspaceProtocol'
| 'storeController'
| 'workspacePackages'
>
& Pick<GetContextOptions, 'storeDir'>

export async function listPeerDependencyIssues (
  projects: ProjectOptions[],
  opts: ListMissingPeersOptions
) {
  const lockfileDir = opts.lockfileDir ?? process.cwd()
  const ctx = await getContext(projects, {
    force: false,
    forceSharedLockfile: false,
    extraBinPaths: [],
    lockfileDir,
    registries: DEFAULT_REGISTRIES,
    useLockfile: true,
    ...opts,
  })
  const projectsToResolve = ctx.projects.map((project) => ({
    ...project,
    updatePackageManifest: false,
    wantedDependencies: getWantedDependencies(project.manifest),
  }))
  const preferredVersions = ctx.wantedLockfile.packages ? getPreferredVersionsFromLockfile(ctx.wantedLockfile.packages) : undefined
  const {
    peerDependencyIssues,
    waitTillAllFetchingsFinish,
  } = await resolveDependencies(
    projectsToResolve,
    {
      currentLockfile: ctx.currentLockfile,
      defaultUpdateDepth: -1,
      dryRun: true,
      engineStrict: false,
      force: false,
      forceFullResolution: true,
      hooks: {
        readPackage: createReadPackageHook({
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

  const conflicts = getPeerDependencyConflicts(peerDependencyIssues)

  await waitTillAllFetchingsFinish()

  return {
    issues: peerDependencyIssues,
    conflicts,
  }
}

function getPeerDependencyConflicts (peerDependencyIssues: PeerDependencyIssues) {
  const missingPeers = new Map<string, string[]>()
  for (const [peerName, issues] of Object.entries(peerDependencyIssues.missing)) {
    missingPeers.set(peerName, issues.map(({ wantedRange }) => wantedRange))
  }
  const conflicts = [] as string[]
  for (const [peerName, ranges] of missingPeers) {
    if (!intersectSafe(ranges)) {
      conflicts.push(peerName)
    }
  }
  return conflicts
}

function intersectSafe (ranges: string[]) {
  try {
    return intersect(...ranges)
  } catch {
    return false
  }
}
