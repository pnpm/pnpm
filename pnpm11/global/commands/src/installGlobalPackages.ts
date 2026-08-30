import { tryReadProjectManifest } from '@pnpm/cli.utils'
import { mutateModulesInSingleProject } from '@pnpm/installing.deps-installer'
import { getRangeSpecStyle } from '@pnpm/pkg-manifest.utils'
import { createStoreController, type CreateStoreControllerOptions } from '@pnpm/store.connection-manager'
import type { IgnoredBuilds, IncludedDependencies, ProjectId, ProjectRootDir } from '@pnpm/types'

export interface ResolutionPolicyViolation {
  name: string
  version: string
  code: string
  reason: string
}

export interface InstallGlobalPackagesResult {
  ignoredBuilds: IgnoredBuilds | undefined
  resolutionPolicyViolations: ResolutionPolicyViolation[]
  /**
   * The version each direct dependency resolved to. Empty when the installer
   * reported no lockfile, which it only does for a mutation that resolved
   * nothing.
   */
  resolvedVersions: Record<string, string>
}

export interface InstallGlobalPackagesOptions extends CreateStoreControllerOptions {
  bin: string
  dir: string
  global?: boolean
  lockfileDir: string
  lockfileOnly?: boolean
  allowBuilds?: Record<string, string | boolean>
  include: IncludedDependencies
  includeDirect?: IncludedDependencies
  omitSummaryLog?: boolean
  rootProjectManifest?: unknown
  rootProjectManifestDir?: string
  saveDev?: boolean
  saveExact?: boolean
  saveOptional?: boolean
  savePeer?: boolean
  savePrefix?: string
  saveProd?: boolean
  sharedWorkspaceLockfile?: boolean
  workspaceDir?: string
  handleResolutionPolicyViolations?: (violations: readonly ResolutionPolicyViolation[]) => Promise<void>
}

export async function installGlobalPackages (
  opts: InstallGlobalPackagesOptions,
  params: string[]
): Promise<InstallGlobalPackagesResult> {
  const store = await createStoreController(opts)
  let { manifest, writeProjectManifest } = await tryReadProjectManifest(opts.dir, opts)
  if (manifest == null) {
    manifest = {}
  }
  const installOpts = {
    ...opts,
    allowBuilds: { ...opts.allowBuilds },
    storeController: store.ctrl,
    storeDir: store.dir,
  }
  const rangeSpecStyle = getRangeSpecStyle(opts)
  const { updatedProject, ignoredBuilds, resolutionPolicyViolations, newLockfile } = await mutateModulesInSingleProject(
    {
      allowNew: true,
      binsDir: opts.bin,
      dependencySelectors: params,
      manifest,
      mutation: 'installSome' as const,
      peer: false,
      rangeSpecStyle,
      rootDir: opts.dir as ProjectRootDir,
      targetDependenciesField: 'dependencies' as const,
    },
    installOpts
  )
  await writeProjectManifest(updatedProject.manifest)
  return {
    ignoredBuilds,
    resolutionPolicyViolations,
    resolvedVersions: resolvedDirectVersions(newLockfile?.importers['.' as ProjectId]?.dependencies),
  }
}

/**
 * The version behind each direct dependency of a lockfile importer. A resolution
 * carries the peers it was made for as a `(peer@version)` suffix; only the
 * version in front of it identifies the release.
 */
function resolvedDirectVersions (
  dependencies: Record<string, string> | undefined
): Record<string, string> {
  const versions: Record<string, string> = {}
  for (const [alias, resolution] of Object.entries(dependencies ?? {})) {
    versions[alias] = resolution.split('(')[0]
  }
  return versions
}
