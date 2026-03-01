import { tryReadProjectManifest } from '@pnpm/cli-utils'
import { getOptionsFromRootManifest } from '@pnpm/config'
import { mutateModulesInSingleProject } from '@pnpm/core'
import { createStoreController, type CreateStoreControllerOptions } from '@pnpm/store-connection-manager'
import { type IgnoredBuilds, type IncludedDependencies, type ProjectRootDir } from '@pnpm/types'

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
}

export async function installGlobalPackages (
  opts: InstallGlobalPackagesOptions,
  params: string[]
): Promise<IgnoredBuilds | undefined> {
  const store = await createStoreController(opts)
  let { manifest, writeProjectManifest } = await tryReadProjectManifest(opts.dir, opts)
  if (manifest == null) {
    manifest = {}
  }
  const rootManifestOpts = getOptionsFromRootManifest(opts.dir, manifest)
  const installOpts = {
    ...opts,
    ...rootManifestOpts,
    allowBuilds: { ...rootManifestOpts.allowBuilds, ...opts.allowBuilds },
    storeController: store.ctrl,
    storeDir: store.dir,
  }
  const pinnedVersion = opts.saveExact ? 'patch' : (opts.savePrefix === '~' ? 'minor' : 'major')
  const { updatedProject, ignoredBuilds } = await mutateModulesInSingleProject(
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
  return ignoredBuilds
}
