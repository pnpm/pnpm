import { tryReadProjectManifest } from '@pnpm/cli.utils'
import { mutateModulesInSingleProject } from '@pnpm/installing.deps-installer'
import { createStoreController, type CreateStoreControllerOptions } from '@pnpm/store.connection-manager'
import type { IgnoredBuilds, IncludedDependencies, ProjectRootDir } from '@pnpm/types'

export interface ResolutionPolicyViolation {
  name: string
  version: string
  code: string
  reason: string
}

export interface InstallGlobalPackagesResult {
  ignoredBuilds: IgnoredBuilds | undefined
  resolutionPolicyViolations: ResolutionPolicyViolation[]
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
  fetchFullMetadata?: boolean
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
  const pinnedVersion = opts.saveExact ? 'patch' : (opts.savePrefix === '~' ? 'minor' : 'major')
  const { updatedProject, ignoredBuilds, resolutionPolicyViolations } = await mutateModulesInSingleProject(
    {
      allowNew: true,
      binsDir: opts.bin,
      dependencySelectors: params,
      manifest,
      mutation: 'installSome' as const,
      peer: false,
      pinnedVersion,
      rootDir: opts.dir as ProjectRootDir,
      targetDependenciesField: 'dependencies' as const,
    },
    installOpts
  )
  await writeProjectManifest(updatedProject.manifest)
  return { ignoredBuilds, resolutionPolicyViolations }
}
