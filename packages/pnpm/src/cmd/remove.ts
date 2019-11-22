import { docsUrl } from '@pnpm/cli-utils'
import { types as allTypes } from '@pnpm/config'
import { oneLine } from 'common-tags'
import R = require('ramda')
import renderHelp = require('render-help')
import {
  mutateModules,
} from 'supi'
import createStoreController from '../createStoreController'
import findWorkspacePackages, { arrayOfLocalPackagesToMap } from '../findWorkspacePackages'
import readImporterManifest from '../readImporterManifest'
import requireHooks from '../requireHooks'
import { PnpmOptions } from '../types'
import { FILTERING, OPTIONS, UNIVERSAL_OPTIONS } from './help'

export function types () {
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
  opts: PnpmOptions,
) {
  const store = await createStoreController(opts)
  const removeOpts = Object.assign(opts, {
    storeController: store.ctrl,
    storeDir: store.dir,
  })
  if (!opts.ignorePnpmfile) {
    opts.hooks = requireHooks(opts.lockfileDir || opts.dir, opts)
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
