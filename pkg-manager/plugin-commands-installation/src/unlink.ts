import { docsUrl, readProjectManifestOnly } from '@pnpm/cli-utils'
import { UNIVERSAL_OPTIONS } from '@pnpm/common-cli-options-help'
import { type Config, getOptionsFromRootManifest } from '@pnpm/config'
import { createOrConnectStoreController, type CreateStoreControllerOptions } from '@pnpm/store-connection-manager'
import { mutateModulesInSingleProject } from '@pnpm/core'
import { type ProjectRootDir } from '@pnpm/types'
import renderHelp from 'render-help'
import { cliOptionsTypes, rcOptionsTypes } from './install'
import { recursive } from './recursive'

export { cliOptionsTypes, rcOptionsTypes }

export const commandNames = ['unlink', 'dislink']

export function help (): string {
  return renderHelp({
    aliases: ['dislink'],
    description: 'Removes the link created by `pnpm link` and reinstalls package if it is saved in `package.json`',
    descriptionLists: [
      {
        title: 'Options',

        list: [
          {
            description: 'Unlink in every package found in subdirectories \
or in every workspace package, when executed inside a workspace. \
For options that may be used with `-r`, see "pnpm help recursive"',
            name: '--recursive',
            shortAlias: '-r',
          },
          ...UNIVERSAL_OPTIONS,
        ],
      },
    ],
    url: docsUrl('unlink'),
    usages: [
      'pnpm unlink (in package dir)',
      'pnpm unlink <pkg>...',
    ],
  })
}

export async function handler (
  opts: CreateStoreControllerOptions &
  Pick<Config,
  | 'allProjects'
  | 'allProjectsGraph'
  | 'bail'
  | 'bin'
  | 'engineStrict'
  | 'hooks'
  | 'linkWorkspacePackages'
  | 'saveWorkspaceProtocol'
  | 'selectedProjectsGraph'
  | 'rawLocalConfig'
  | 'registries'
  | 'rootProjectManifest'
  | 'rootProjectManifestDir'
  | 'pnpmfile'
  | 'workspaceDir'
  > & {
    recursive?: boolean
  },
  params: string[]
): Promise<void> {
  if (opts.recursive && (opts.allProjects != null) && (opts.selectedProjectsGraph != null) && opts.workspaceDir) {
    await recursive(opts.allProjects, params, {
      ...opts,
      allProjectsGraph: opts.allProjectsGraph!,
      selectedProjectsGraph: opts.selectedProjectsGraph,
      workspaceDir: opts.workspaceDir,
    }, 'unlink')
    return
  }
  const store = await createOrConnectStoreController(opts)
  const unlinkOpts = Object.assign(opts, {
    ...getOptionsFromRootManifest(opts.rootProjectManifestDir, opts.rootProjectManifest ?? {}),
    globalBin: opts.bin,
    storeController: store.ctrl,
    storeDir: store.dir,
  })

  if (!params || (params.length === 0)) {
    await mutateModulesInSingleProject({
      dependencyNames: params,
      manifest: await readProjectManifestOnly(opts.dir, opts),
      mutation: 'unlinkSome',
      rootDir: opts.dir as ProjectRootDir,
    }, unlinkOpts)
    return
  }
  await mutateModulesInSingleProject({
    manifest: await readProjectManifestOnly(opts.dir, opts),
    mutation: 'unlink',
    rootDir: opts.dir as ProjectRootDir,
  }, unlinkOpts)
}
