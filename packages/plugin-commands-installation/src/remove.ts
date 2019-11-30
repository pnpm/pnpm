import { docsUrl, readImporterManifest } from '@pnpm/cli-utils'
import { FILTERING, OPTIONS, UNIVERSAL_OPTIONS } from '@pnpm/common-cli-options-help'
import { Config, types as allTypes } from '@pnpm/config'
import findWorkspacePackages, { arrayOfLocalPackagesToMap } from '@pnpm/find-workspace-packages'
import { requireHooks } from '@pnpm/pnpmfile'
import { createOrConnectStoreController, CreateStoreControllerOptions } from '@pnpm/store-connection-manager'
import { oneLine } from 'common-tags'
import R = require('ramda')
import renderHelp = require('render-help')
import {
  mutateModules,
} from 'supi'

export const rcOptionsTypes = cliOptionsTypes

export function cliOptionsTypes () {
  return R.pick([
    'force',
    'global-dir',
    'global-pnpmfile',
    'global',
    'lockfile-dir',
    'lockfile-directory',
    'lockfile-only',
    'lockfile',
    'package-import-method',
    'pnpmfile',
    'recursive',
    'reporter',
    'resolution-strategy',
    'shared-workspace-lockfile',
    'store',
    'store-dir',
    'virtual-store-dir',
  ], allTypes)
}

export function help () {
  return renderHelp({
    aliases: ['rm', 'r', 'uninstall', 'un'],
    description: `Removes packages from \`node_modules\` and from the project's \`packages.json\`.`,
    descriptionLists: [
      {
        title: 'Options',

        list: [
          {
            description: oneLine`
              Remove from every package found in subdirectories
              or from every workspace package, when executed inside a workspace.
              For options that may be used with \`-r\`, see "pnpm help recursive"
            `,
            name: '--recursive',
            shortAlias: '-r',
          },
          OPTIONS.globalDir,
          ...UNIVERSAL_OPTIONS,
        ],
      },
      FILTERING,
    ],
    url: docsUrl('remove'),
    usages: ['pnpm remove <pkg>[@<version>]...'],
  })
}

export const commandNames = ['remove', 'uninstall', 'r', 'rm', 'un']

export async function handler (
  input: string[],
  opts: CreateStoreControllerOptions & Pick<Config, 'ignorePnpmfile' | 'engineStrict' | 'lockfileDir' | 'linkWorkspacePackages' | 'workspaceDir' | 'bin' | 'globalPnpmfile' | 'pnpmfile'>,
) {
  const store = await createOrConnectStoreController(opts)
  const removeOpts = Object.assign(opts, {
    storeController: store.ctrl,
    storeDir: store.dir,
  })
  if (!opts.ignorePnpmfile) {
    removeOpts['hooks'] = requireHooks(opts.lockfileDir || opts.dir, opts)
  }
  removeOpts['localPackages'] = opts.linkWorkspacePackages && opts.workspaceDir
    ? arrayOfLocalPackagesToMap(await findWorkspacePackages(opts.workspaceDir, opts))
    : undefined
  const currentManifest = await readImporterManifest(opts.dir, opts)
  const [mutationResult] = await mutateModules(
    [
      {
        binsDir: opts.bin,
        dependencyNames: input,
        manifest: currentManifest.manifest,
        mutation: 'uninstallSome',
        rootDir: opts.dir,
      },
    ],
    removeOpts,
  )
  await currentManifest.writeImporterManifest(mutationResult.manifest)
}
