import {
  docsUrl,
  getSaveType,
  optionTypesToCompletions,
  readDepNameCompletions,
  readProjectManifest,
} from '@pnpm/cli-utils'
import { CompletionFunc } from '@pnpm/command'
import { FILTERING, OPTIONS, UNIVERSAL_OPTIONS } from '@pnpm/common-cli-options-help'
import { Config, types as allTypes } from '@pnpm/config'
import findWorkspacePackages, { arrayOfWorkspacePackagesToMap } from '@pnpm/find-workspace-packages'
import { requireHooks } from '@pnpm/pnpmfile'
import { createOrConnectStoreController, CreateStoreControllerOptions } from '@pnpm/store-connection-manager'
import { getAllDependenciesFromPackage } from '@pnpm/utils'
import { oneLine } from 'common-tags'
import R = require('ramda')
import renderHelp = require('render-help')
import {
  mutateModules,
} from 'supi'
import recursive from './recursive'

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
    'save-dev',
    'save-optional',
    'save-prod',
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
          {
            description: 'Remove the dependency only from "devDependencies"',
            name: '--save-dev',
            shortAlias: '-D',
          },
          {
            description: 'Remove the dependency only from "optionalDependencies"',
            name: '--save-optional',
            shortAlias: '-O',
          },
          {
            description: 'Remove the dependency only from "dependencies"',
            name: '--save-prod',
            shortAlias: '-P',
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

export const completion: CompletionFunc = (args, cliOpts) => {
  return readDepNameCompletions(cliOpts.dir as string)
}

export async function handler (
  input: string[],
  opts: CreateStoreControllerOptions & Pick<Config,
    'allProjects' |
    'bail' |
    'bin' |
    'engineStrict' |
    'globalPnpmfile' |
    'ignorePnpmfile' |
    'lockfileDir' |
    'linkWorkspacePackages' |
    'pnpmfile' |
    'rawLocalConfig' |
    'registries' |
    'saveDev' |
    'saveOptional' |
    'saveProd' |
    'selectedProjectsGraph' |
    'workspaceDir'
  > & {
    recursive?: boolean,
  },
) {
  if (opts.recursive && opts.allProjects && opts.selectedProjectsGraph && opts.workspaceDir) {
    await recursive(opts.allProjects, input, { ...opts, selectedProjectsGraph: opts.selectedProjectsGraph!, workspaceDir: opts.workspaceDir! }, 'remove')
    return
  }
  const store = await createOrConnectStoreController(opts)
  const removeOpts = Object.assign(opts, {
    storeController: store.ctrl,
    storeDir: store.dir,
  })
  if (!opts.ignorePnpmfile) {
    removeOpts['hooks'] = requireHooks(opts.lockfileDir || opts.dir, opts)
  }
  removeOpts['workspacePackages'] = opts.workspaceDir
    ? arrayOfWorkspacePackagesToMap(await findWorkspacePackages(opts.workspaceDir, opts))
    : undefined
  const currentManifest = await readProjectManifest(opts.dir, opts)
  const [mutationResult] = await mutateModules(
    [
      {
        binsDir: opts.bin,
        dependencyNames: input,
        manifest: currentManifest.manifest,
        mutation: 'uninstallSome',
        rootDir: opts.dir,
        targetDependenciesField: getSaveType(opts),
      },
    ],
    removeOpts,
  )
  await currentManifest.writeProjectManifest(mutationResult.manifest)
}
