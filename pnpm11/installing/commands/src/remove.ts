import type { CompletionFunc } from '@pnpm/cli.command'
import { FILTERING, OPTIONS, UNIVERSAL_OPTIONS } from '@pnpm/cli.common-cli-options-help'
import {
  docsUrl,
  readDepNameCompletions,
  readProjectManifest,
} from '@pnpm/cli.utils'
import { type Config, type ConfigContext, types as allTypes } from '@pnpm/config.reader'
import { PnpmError } from '@pnpm/error'
import { handleGlobalRemove } from '@pnpm/global.commands'
import { arrayOfWorkspacePackagesToMap } from '@pnpm/installing.context'
import { mutateModulesInSingleProject } from '@pnpm/installing.deps-installer'
import type { LockfileObject } from '@pnpm/lockfile.types'
import { getAllDependenciesFromManifest } from '@pnpm/pkg-manifest.utils'
import { createStoreController, type CreateStoreControllerOptions, type StoreControllerHandle } from '@pnpm/store.connection-manager'
import type { DependenciesField, IncludedDependencies, Project, ProjectManifest, ProjectRootDir } from '@pnpm/types'
import { findWorkspaceProjects } from '@pnpm/workspace.projects-reader'
import { updateWorkspaceManifest } from '@pnpm/workspace.workspace-manifest-writer'
import { pick, without } from 'ramda'
import { renderHelp } from 'render-help'

import { getSaveType } from './getSaveType.js'
import { recursive } from './recursive.js'
import { keptCatalogsForPrune, resolvedPackageVersionsForPrune } from './resolvedPackageVersionsForPrune.js'

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

const RC_OPTION_NAMES = [
  'cache-dir',
  'global-dir',
  'global-pnpmfile',
  'global',
  'lockfile-dir',
  'lockfile-only',
  'lockfile',
  'node-experimental-package-map',
  'node-package-map-type',
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
  'trust-lockfile',
  'trust-policy',
  'trust-policy-exclude',
  'trust-policy-ignore-after',
  'unsafe-perm',
  'virtual-store-dir',
] as const

export function rcOptionsTypes (): Record<string, unknown> {
  return pick(RC_OPTION_NAMES, allTypes)
}

export const cliOptionsTypes = (): Record<string, unknown> => ({
  ...rcOptionsTypes(),
  ...pick(['force'], allTypes),
  recursive: Boolean,
})

const OPTIONS_HELP_LIST = [
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
  {
    description: 'Trust the lockfile and skip the supply-chain verification step that re-applies minimumReleaseAge / trustPolicy to each lockfile entry. Use only when the lockfile is part of the trusted base (closed-source projects, CI runs against an already-verified lockfile)',
    name: '--trust-lockfile',
  },
  OPTIONS.globalDir,
  ...UNIVERSAL_OPTIONS,
]

export function help (): string {
  return renderHelp({
    aliases: ['rm', 'uninstall', 'un'],
    description: 'Removes packages from `node_modules` and from the project\'s `package.json`.',
    descriptionLists: [
      {
        title: 'Options',

        list: OPTIONS_HELP_LIST,
      },
      FILTERING,
    ],
    url: docsUrl('remove'),
    usages: ['pnpm remove <pkg>...'],
  })
}

// Unlike npm, pnpm does not treat "r" as an alias of "remove".
// This way we avoid the confusion about whether "pnpm r" means remove, run, or recursive.
export const commandNames = ['remove', 'uninstall', 'rm', 'un', 'uni']

export const completion: CompletionFunc = async (cliOpts) => {
  return readDepNameCompletions(cliOpts.dir as string)
}

export type RemoveCommandOptions = CreateStoreControllerOptions & Pick<Config,
| 'bail'
| 'bin'
| 'configDependencies'
| 'dev'
| 'engineStrict'
| 'globalPnpmfile'
| 'ignorePnpmfile'
| 'linkWorkspacePackages'
| 'lockfileDir'
| 'optional'
| 'production'
| 'registriesByScope'
| 'saveDev'
| 'saveOptional'
| 'saveProd'
| 'workspaceDir'
| 'workspacePackagePatterns'
| 'sharedWorkspaceLockfile'
| 'lockfile'
| 'catalogPrune'
| 'minimumReleaseAgeExcludePrune'
| 'trustPolicyExcludePrune'
| 'trustLockfile'
> & Pick<ConfigContext,
| 'allProjects'
| 'allProjectsGraph'
| 'hooks'
| 'rootProjectManifest'
| 'rootProjectManifestDir'
| 'selectedProjectsGraph'
> & {
  recursive?: boolean
  pnpmfile: string[]
} & Partial<Pick<Config, 'global' | 'globalPkgDir'>>

export async function handler (
  opts: RemoveCommandOptions,
  params: string[]
): Promise<void> {
  if (params.length === 0) throw new PnpmError('MUST_REMOVE_SOMETHING', 'At least one dependency name should be specified for removal')
  if (opts.global) {
    return removeGlobally(opts, params)
  }
  const include = {
    dependencies: opts.production !== false,
    devDependencies: opts.dev !== false,
    optionalDependencies: opts.optional !== false,
  }
  const store = await createStoreController(opts)
  if (opts.recursive && (opts.allProjects != null) && (opts.selectedProjectsGraph != null) && opts.workspaceDir) {
    await removeRecursively({
      opts,
      params,
      include,
      store,
      allProjects: opts.allProjects,
      selectedProjectsGraph: opts.selectedProjectsGraph,
      workspaceDir: opts.workspaceDir,
    })
    return
  }
  await removeFromProject({ opts, params, include, store })
}

type SelectedProjectsGraph = NonNullable<RemoveCommandOptions['selectedProjectsGraph']>

interface RemoveContext {
  opts: RemoveCommandOptions
  params: string[]
  include: IncludedDependencies
  store: StoreControllerHandle
}

async function removeGlobally (opts: RemoveCommandOptions, params: string[]): Promise<void> {
  if (!opts.bin) {
    throw new PnpmError('NO_GLOBAL_BIN_DIR', 'Unable to find the global bin directory', {
      hint: 'Run "pnpm setup" to create it automatically, or set the global-bin-dir setting, or the PNPM_HOME env variable. The global bin directory should be in the PATH.',
    })
  }
  return handleGlobalRemove(opts, params)
}

interface RecursiveRemoveContext extends RemoveContext {
  allProjects: Project[]
  selectedProjectsGraph: SelectedProjectsGraph
  workspaceDir: string
}

async function removeRecursively (
  { opts, params, include, store, allProjects, selectedProjectsGraph, workspaceDir }: RecursiveRemoveContext
): Promise<void> {
  if (Object.keys(selectedProjectsGraph).length === 0) return
  const targetDependenciesField = getSaveType(opts)
  const availableDependencies = collectDependenciesOfSelectedProjects({
    allProjects,
    selectedProjectsGraph,
    targetDependenciesField,
  })
  assertDependenciesToRemoveExist({ availableDependencies, params, targetDependenciesField })
  await recursive(allProjects, params, {
    ...opts,
    allProjectsGraph: opts.allProjectsGraph!,
    include,
    selectedProjectsGraph,
    storeControllerAndDir: store,
    workspaceDir,
  }, 'remove')
}

function collectDependenciesOfSelectedProjects (opts: {
  allProjects: Project[]
  selectedProjectsGraph: SelectedProjectsGraph
  targetDependenciesField: DependenciesField | undefined
}): string[] {
  const availableDependenciesSet = new Set<string>()
  for (const project of opts.allProjects) {
    if (!opts.selectedProjectsGraph[project.rootDir as ProjectRootDir]) continue
    const deps = Object.keys(
      opts.targetDependenciesField === undefined
        ? getAllDependenciesFromManifest(project.manifest, { autoInstallPeers: true })
        : project.manifest[opts.targetDependenciesField] ?? {}
    )
    for (const dep of deps) {
      availableDependenciesSet.add(dep)
    }
  }
  return Array.from(availableDependenciesSet).sort()
}

function assertDependenciesToRemoveExist (opts: {
  availableDependencies: string[]
  params: string[]
  targetDependenciesField: DependenciesField | undefined
}): void {
  const nonMatchedDependencies = without(opts.availableDependencies, opts.params)
  if (nonMatchedDependencies.length !== 0) {
    throw new RemoveMissingDepsError({
      availableDependencies: opts.availableDependencies,
      nonMatchedDependencies,
      targetDependenciesField: opts.targetDependenciesField,
    })
  }
}

async function removeFromProject ({ opts, params, include, store }: RemoveContext): Promise<void> {
  const removeOpts = Object.assign(opts, {
    linkWorkspacePackagesDepth: opts.linkWorkspacePackages === 'deep' ? Infinity : opts.linkWorkspacePackages ? 0 : -1,
    storeController: store.ctrl,
    storeDir: store.dir,
    resolutionVerifiers: store.resolutionVerifiers,
    include,
    // `--dry-run` is an `install`-only preview; never let a config-level
    // `dry-run` turn `remove` into a no-op check.
    dryRun: false,
  })
  const allProjects = opts.allProjects ?? await readWorkspaceProjects(opts)
  // @ts-expect-error -- workspacePackages is an install option that the remove options type does not declare
  removeOpts['workspacePackages'] = allProjects
    ? arrayOfWorkspacePackagesToMap(allProjects)
    : undefined
  const targetDependenciesField = getSaveType(opts)
  const {
    manifest: currentManifest,
    writeProjectManifest,
  } = await readProjectManifest(opts.dir, opts)
  assertDependenciesToRemoveExist({
    availableDependencies: listDependencyNames(currentManifest, targetDependenciesField),
    params,
    targetDependenciesField,
  })
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

  await pruneWorkspaceManifest(opts, {
    allProjects: replaceUpdatedProjectManifest(allProjects ?? [], mutationResult.updatedProject),
    newLockfile: mutationResult.newLockfile,
    wantedLockfile: mutationResult.wantedLockfile,
  })
}

async function pruneWorkspaceManifest (
  opts: RemoveCommandOptions,
  { allProjects, newLockfile, wantedLockfile }: { allProjects: Project[], newLockfile: LockfileObject | undefined, wantedLockfile: LockfileObject | undefined }
): Promise<void> {
  await updateWorkspaceManifest(opts.workspaceDir ?? opts.dir, {
    catalogPrune: opts.catalogPrune,
    keptCatalogs: keptCatalogsForPrune(opts, newLockfile, wantedLockfile),
    resolvedPackageVersions: resolvedPackageVersionsForPrune(opts, newLockfile),
    minimumReleaseAgeExcludePrune: opts.minimumReleaseAgeExcludePrune,
    trustPolicyExcludePrune: opts.trustPolicyExcludePrune,
    allProjects,
  })
}

function listDependencyNames (
  manifest: ProjectManifest,
  targetDependenciesField: DependenciesField | undefined
): string[] {
  return Object.keys(
    targetDependenciesField === undefined
      ? getAllDependenciesFromManifest(manifest)
      : manifest[targetDependenciesField] ?? {}
  )
}

async function readWorkspaceProjects (opts: RemoveCommandOptions): Promise<Project[] | undefined> {
  return opts.workspaceDir
    ? findWorkspaceProjects(opts.workspaceDir, { ...opts, patterns: opts.workspacePackagePatterns })
    : undefined
}

function replaceUpdatedProjectManifest (
  allProjects: Project[],
  updatedProject: Pick<Project, 'manifest' | 'rootDir'>
): Project[] {
  return allProjects.map((project) => project.rootDir === updatedProject.rootDir
    ? { ...project, manifest: updatedProject.manifest }
    : project
  )
}
