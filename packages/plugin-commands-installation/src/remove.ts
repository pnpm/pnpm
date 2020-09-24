import {
  docsUrl,
  readDepNameCompletions,
  readProjectManifest,
} from '@pnpm/cli-utils'
import { CompletionFunc } from '@pnpm/command'
import { FILTERING, OPTIONS, UNIVERSAL_OPTIONS } from '@pnpm/common-cli-options-help'
import { Config, types as allTypes } from '@pnpm/config'
import PnpmError from '@pnpm/error'
import findWorkspacePackages, { arrayOfWorkspacePackagesToMap } from '@pnpm/find-workspace-packages'
import { getAllDependenciesFromManifest } from '@pnpm/manifest-utils'
import { requireHooks } from '@pnpm/pnpmfile'
import { createOrConnectStoreController, CreateStoreControllerOptions } from '@pnpm/store-connection-manager'
import { DependenciesField } from '@pnpm/types'
import {
  mutateModules,
} from 'supi'
import getSaveType from './getSaveType'
import recursive from './recursive'
import R = require('ramda')
import renderHelp = require('render-help')

class RemoveMissingDepsError extends PnpmError {
  constructor (
    opts: {
      availableDependencies: string[]
      nonMatchedDependencies: string[]
      targetDependenciesField?: DependenciesField
    }
  ) {
    let message = 'Cannot remove '
    message += `${opts.nonMatchedDependencies.map(dep => `'${dep}'`).join(', ')}: `
    if (opts.availableDependencies.length > 0) {
      message += `no such ${opts.nonMatchedDependencies.length > 1 ? 'dependencies' : 'dependency'} `
      message += `found${opts.targetDependenciesField ? ` in '${opts.targetDependenciesField}'` : ''}`
      const hint = `Available dependencies: ${opts.availableDependencies.join(', ')}`
      super('CANNOT_REMOVE_MISSING_DEPS', message, { hint })
      return
    }
    message += opts.targetDependenciesField
      ? `project has no '${opts.targetDependenciesField}'`
      : 'project has no dependencies of any kind'
    super('CANNOT_REMOVE_MISSING_DEPS', message)
  }
}

export function rcOptionsTypes () {
  return R.pick([
    'global-dir',
    'global-pnpmfile',
    'global',
    'lockfile-dir',
    'lockfile-directory',
    'lockfile-only',
    'lockfile',
    'package-import-method',
    'pnpmfile',
    'reporter',
    'save-dev',
    'save-optional',
    'save-prod',
    'shared-workspace-lockfile',
    'store',
    'store-dir',
    'virtual-store-dir',
  ], allTypes)
}

export const cliOptionsTypes = () => ({
  ...rcOptionsTypes(),
  ...R.pick(['force'], allTypes),
  recursive: Boolean,
})

export function help () {
  return renderHelp({
    aliases: ['rm', 'uninstall', 'un'],
    description: 'Removes packages from `node_modules` and from the project\'s `package.json`.',
    descriptionLists: [
      {
        title: 'Options',

        list: [
          {
            description: 'Remove from every package found in subdirectories \
or from every workspace package, when executed inside a workspace. \
For options that may be used with `-r`, see "pnpm help recursive"',
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

// Unlike npm, pnpm does not treat "r" as an alias of "remove".
// This way we avoid the confusion about whether "pnpm r" means remove, run, or recursive.
export const commandNames = ['remove', 'uninstall', 'rm', 'un']

export const completion: CompletionFunc = (cliOpts, params) => {
  return readDepNameCompletions(cliOpts.dir as string)
}

export async function handler (
  opts: CreateStoreControllerOptions & Pick<Config,
  | 'allProjects'
  | 'bail'
  | 'bin'
  | 'engineStrict'
  | 'globalPnpmfile'
  | 'ignorePnpmfile'
  | 'lockfileDir'
  | 'linkWorkspacePackages'
  | 'pnpmfile'
  | 'rawLocalConfig'
  | 'registries'
  | 'saveDev'
  | 'saveOptional'
  | 'saveProd'
  | 'selectedProjectsGraph'
  | 'workspaceDir'
  > & {
    recursive?: boolean
  },
  params: string[]
) {
  if (params.length === 0) throw new PnpmError('MUST_REMOVE_SOMETHING', 'At least one dependency name should be specified for removal')
  if (opts.recursive && opts.allProjects && opts.selectedProjectsGraph && opts.workspaceDir) {
    await recursive(opts.allProjects, params, { ...opts, selectedProjectsGraph: opts.selectedProjectsGraph, workspaceDir: opts.workspaceDir }, 'remove')
    return
  }
  const store = await createOrConnectStoreController(opts)
  const removeOpts = Object.assign(opts, {
    storeController: store.ctrl,
    storeDir: store.dir,
  })
  if (!opts.ignorePnpmfile) {
    removeOpts['hooks'] = requireHooks(opts.lockfileDir ?? opts.dir, opts)
  }
  removeOpts['workspacePackages'] = opts.workspaceDir
    ? arrayOfWorkspacePackagesToMap(await findWorkspacePackages(opts.workspaceDir, opts))
    : undefined
  const targetDependenciesField = getSaveType(opts)
  const {
    manifest: currentManifest,
    writeProjectManifest,
  } = await readProjectManifest(opts.dir, opts)
  const availableDependencies = Object.keys(
    targetDependenciesField === undefined
      ? getAllDependenciesFromManifest(currentManifest)
      : currentManifest[targetDependenciesField] ?? {}
  )
  const nonMatchedDependencies = R.without(availableDependencies, params)
  if (nonMatchedDependencies.length !== 0) {
    throw new RemoveMissingDepsError({
      availableDependencies,
      nonMatchedDependencies,
      targetDependenciesField,
    })
  }
  const [mutationResult] = await mutateModules(
    [
      {
        binsDir: opts.bin,
        dependencyNames: params,
        manifest: currentManifest,
        mutation: 'uninstallSome',
        rootDir: opts.dir,
        targetDependenciesField,
      },
    ],
    removeOpts
  )
  await writeProjectManifest(mutationResult.manifest)
}
