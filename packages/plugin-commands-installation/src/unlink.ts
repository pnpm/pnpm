import { docsUrl, readProjectManifestOnly } from '@pnpm/cli-utils'
import { UNIVERSAL_OPTIONS } from '@pnpm/common-cli-options-help'
import { Config } from '@pnpm/config'
import { createOrConnectStoreController, CreateStoreControllerOptions } from '@pnpm/store-connection-manager'
import { mutateModules } from 'supi'
import { cliOptionsTypes, rcOptionsTypes } from './install'
import recursive from './recursive'
import renderHelp = require('render-help')

export { cliOptionsTypes, rcOptionsTypes }

export const commandNames = ['unlink', 'dislink']

export function help () {
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
  | 'bail'
  | 'bin'
  | 'engineStrict'
  | 'linkWorkspacePackages'
  | 'selectedProjectsGraph'
  | 'rawLocalConfig'
  | 'registries'
  | 'pnpmfile'
  | 'workspaceDir'
  > & {
    recursive?: boolean
  },
  params: string[]
) {
  if (opts.recursive && opts.allProjects && opts.selectedProjectsGraph && opts.workspaceDir) {
    await recursive(opts.allProjects, params, { ...opts, selectedProjectsGraph: opts.selectedProjectsGraph, workspaceDir: opts.workspaceDir }, 'unlink')
    return
  }
  const store = await createOrConnectStoreController(opts)
  const unlinkOpts = Object.assign(opts, {
    globalBin: opts.bin,
    storeController: store.ctrl,
    storeDir: store.dir,
  })

  if (!params || !params.length) {
    return mutateModules([
      {
        dependencyNames: params,
        manifest: await readProjectManifestOnly(opts.dir, opts),
        mutation: 'unlinkSome',
        rootDir: opts.dir,
      },
    ], unlinkOpts)
  }
  return mutateModules([
    {
      manifest: await readProjectManifestOnly(opts.dir, opts),
      mutation: 'unlink',
      rootDir: opts.dir,
    },
  ], unlinkOpts)
}
