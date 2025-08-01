import {
  docsUrl,
  readDepNameCompletions,
  readProjectManifest,
} from '@pnpm/cli-utils'
import { type CompletionFunc } from '@pnpm/command'
import { FILTERING, OPTIONS, UNIVERSAL_OPTIONS } from '@pnpm/common-cli-options-help'
import { type Config, getOptionsFromRootManifest, types as allTypes } from '@pnpm/config'
import { PnpmError } from '@pnpm/error'
import { arrayOfWorkspacePackagesToMap } from '@pnpm/get-context'
import { findWorkspacePackages } from '@pnpm/workspace.find-packages'
import { getAllDependenciesFromManifest } from '@pnpm/manifest-utils'
import { createOrConnectStoreController, type CreateStoreControllerOptions } from '@pnpm/store-connection-manager'
import { type DependenciesField, type ProjectRootDir } from '@pnpm/types'
import { mutateModulesInSingleProject } from '@pnpm/core'
import pick from 'ramda/src/pick'
import without from 'ramda/src/without'
import renderHelp from 'render-help'
import { getSaveType } from './getSaveType'
import { recursive } from './recursive'

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

export function rcOptionsTypes (): Record<string, unknown> {
  return pick([
    'cache-dir',
    'global-dir',
    'global-pnpmfile',
    'global',
    'lockfile-dir',
    'lockfile-directory',
    'lockfile-only',
    'lockfile',
    'node-linker',
    'package-import-method',
    'pnpmfile',
    'reporter',
    'save-dev',
    'save-optional',
    'save-prod',
    'shared-workspace-lockfile',
    'store-dir',
    'strict-peer-dependencies',
    'virtual-store-dir',
  ], allTypes)
}

export const cliOptionsTypes = (): Record<string, unknown> => ({
  ...rcOptionsTypes(),
  ...pick(['force'], allTypes),
  recursive: Boolean,
})

export function help (): string {
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
export const commandNames = ['remove', 'uninstall', 'rm', 'un', 'uni']

export const completion: CompletionFunc = async (cliOpts) => {
  return readDepNameCompletions(cliOpts.dir as string)
}

export async function handler (
  opts: CreateStoreControllerOptions & Pick<Config,
  | 'allProjects'
  | 'allProjectsGraph'
  | 'bail'
  | 'bin'
  | 'configDependencies'
  | 'dev'
  | 'engineStrict'
  | 'globalPnpmfile'
  | 'hooks'
  | 'ignorePnpmfile'
  | 'linkWorkspacePackages'
  | 'lockfileDir'
  | 'optional'
  | 'production'
  | 'rawLocalConfig'
  | 'registries'
  | 'rootProjectManifest'
  | 'rootProjectManifestDir'
  | 'saveDev'
  | 'saveOptional'
  | 'saveProd'
  | 'selectedProjectsGraph'
  | 'workspaceDir'
  | 'workspacePackagePatterns'
  | 'sharedWorkspaceLockfile'
  > & {
    recursive?: boolean
    pnpmfile: string[]
  },
  params: string[]
): Promise<void> {
  if (params.length === 0) throw new PnpmError('MUST_REMOVE_SOMETHING', 'At least one dependency name should be specified for removal')
  const include = {
    dependencies: opts.production !== false,
    devDependencies: opts.dev !== false,
    optionalDependencies: opts.optional !== false,
  }
  const store = await createOrConnectStoreController(opts)
  if (opts.recursive && (opts.allProjects != null) && (opts.selectedProjectsGraph != null) && opts.workspaceDir) {
    await recursive(opts.allProjects, params, {
      ...opts,
      allProjectsGraph: opts.allProjectsGraph!,
      include,
      selectedProjectsGraph: opts.selectedProjectsGraph,
      storeControllerAndDir: store,
      workspaceDir: opts.workspaceDir,
    }, 'remove')
    return
  }
  const removeOpts = Object.assign(opts, {
    ...getOptionsFromRootManifest(opts.rootProjectManifestDir, opts.rootProjectManifest ?? {}),
    linkWorkspacePackagesDepth: opts.linkWorkspacePackages === 'deep' ? Infinity : opts.linkWorkspacePackages ? 0 : -1,
    storeController: store.ctrl,
    storeDir: store.dir,
    include,
  })
  // @ts-expect-error
  removeOpts['workspacePackages'] = opts.workspaceDir
    ? arrayOfWorkspacePackagesToMap(await findWorkspacePackages(opts.workspaceDir, { ...opts, patterns: opts.workspacePackagePatterns }))
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
  const nonMatchedDependencies = without(availableDependencies, params)
  if (nonMatchedDependencies.length !== 0) {
    throw new RemoveMissingDepsError({
      availableDependencies,
      nonMatchedDependencies,
      targetDependenciesField,
    })
  }
  const mutationResult = await mutateModulesInSingleProject(
    {
      binsDir: opts.bin,
      dependencyNames: params,
      manifest: currentManifest,
      mutation: 'uninstallSome',
      rootDir: opts.dir as ProjectRootDir,
      targetDependenciesField,
    },
    removeOpts
  )
  await writeProjectManifest(mutationResult.updatedProject.manifest)
}
