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
import { handleGlobalRemove } from '@pnpm/global.commands'
import { findWorkspacePackages } from '@pnpm/workspace.find-packages'
import { updateWorkspaceManifest } from '@pnpm/workspace.manifest-writer'
import { getAllDependenciesFromManifest } from '@pnpm/manifest-utils'
import { createStoreController, type CreateStoreControllerOptions } from '@pnpm/store-connection-manager'
import { type DependenciesField, type ProjectRootDir, type Project } from '@pnpm/types'
import { mutateModulesInSingleProject } from '@pnpm/core'
import { pick, without } from 'ramda'
import renderHelp from 'render-help'
import { getSaveType } from './getSaveType.js'
import { recursive } from './recursive.js'

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
  | 'cleanupUnusedCatalogs'
  > & {
    recursive?: boolean
    pnpmfile: string[]
  } & Partial<Pick<Config, 'global' | 'globalPkgDir'>>,
  params: string[]
): Promise<void> {
  if (params.length === 0) throw new PnpmError('MUST_REMOVE_SOMETHING', 'At least one dependency name should be specified for removal')
  if (opts.global) {
    if (!opts.bin) {
      throw new PnpmError('NO_GLOBAL_BIN_DIR', 'Unable to find the global bin directory', {
        hint: 'Run "pnpm setup" to create it automatically, or set the global-bin-dir setting, or the PNPM_HOME env variable. The global bin directory should be in the PATH.',
      })
    }
    return handleGlobalRemove(opts, params)
  }
  const include = {
    dependencies: opts.production !== false,
    devDependencies: opts.dev !== false,
    optionalDependencies: opts.optional !== false,
  }
  const store = await createStoreController(opts)
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
  const allProjects = opts.allProjects ?? (
    opts.workspaceDir
      ? await findWorkspacePackages(opts.workspaceDir, { ...opts, patterns: opts.workspacePackagePatterns })
      : undefined
  )
  // @ts-expect-error
  removeOpts['workspacePackages'] = allProjects
    ? arrayOfWorkspacePackagesToMap(allProjects)
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

  const updatedProjects: Project[] = []
  if (allProjects != null) {
    for (const project of allProjects) {
      if (project.rootDir === mutationResult.updatedProject.rootDir) {
        updatedProjects.push({
          ...project,
          manifest: mutationResult.updatedProject.manifest,
        })
      } else {
        updatedProjects.push(project)
      }
    }
  }
  await updateWorkspaceManifest(opts.workspaceDir ?? opts.dir, {
    cleanupUnusedCatalogs: opts.cleanupUnusedCatalogs,
    allProjects: updatedProjects,
  })
}
