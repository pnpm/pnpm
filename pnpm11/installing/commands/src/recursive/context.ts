import { createProjectConfigRecord, type ProjectConfig } from '@pnpm/config.reader'
import { PnpmError } from '@pnpm/error'
import { arrayOfWorkspacePackagesToMap } from '@pnpm/installing.context'
import type { InstallOptions, ProjectOptions, WorkspacePackages } from '@pnpm/installing.deps-installer'
import { filterDependenciesByType, getRangeSpecStyle } from '@pnpm/pkg-manifest.utils'
import { createStoreController } from '@pnpm/store.connection-manager'
import type {
  DependenciesField,
  IncludedDependencies,
  Project,
  ProjectManifest,
  ProjectRootDir,
  ProjectsGraph,
  RangeSpecStyle,
} from '@pnpm/types'
import { projectsDependencies } from '@pnpm/workspace.projects-sorter'

import { getSaveType } from '../getSaveType.js'
import { setupPolicyHandlers } from '../policyHandlers.js'
import { toWorkspaceSpecs } from '../updateWorkspaceDependencies.js'
import type { CommandFullName, RecursiveOptions } from './options.js'
import {
  createMatcher,
  failOnVersionsOfIndirectUpdateSpecs,
  makeIgnorePatterns,
  matchDependencies,
  type UpdateDepsMatcher,
} from './updateSelectors.js'

export type ManifestsByPath = Record<string, Omit<Project, 'rootDir' | 'rootDirRealPath'>>

export type StoreControllerAndDir = NonNullable<RecursiveOptions['storeControllerAndDir']>

export type PolicyHandlers = ReturnType<typeof setupPolicyHandlers>

export interface RecursiveContext {
  allProjects: Project[]
  cmdFullName: CommandFullName
  /**
   * The selectors the user passed, or for an `update` without selectors, the
   * negated `updateConfig.ignoreDependencies`.
   */
  params: string[]
  /**
   * Whether the user named any package. `params` is rewritten per project into
   * the dependency names it matched, and `--workspace` only insists that a
   * dependency exists in the workspace when it was asked for by name.
   */
  userNamedDeps: boolean
  /** The same object as `installOpts`, which extends it in place. */
  opts: RecursiveOptions
  installOpts: InstallOptions
  manifestsByPath: ManifestsByPath
  store: StoreControllerAndDir
  workspacePackages: WorkspacePackages
  targetDependenciesField: DependenciesField | undefined
  policyHandlers: PolicyHandlers
  getProjectConfig: (manifest: Pick<ProjectManifest, 'name'>) => ProjectConfig | undefined
  updateToLatest: boolean | undefined
  includeDirect: IncludedDependencies
  updateMatch: UpdateDepsMatcher | null
}

export interface RecursiveInput {
  allProjects: Project[]
  selectedProjects: Array<Pick<Project, 'manifest'>>
  params: string[]
  opts: RecursiveOptions
  cmdFullName: CommandFullName
}

export async function createRecursiveContext (input: RecursiveInput): Promise<RecursiveContext> {
  const { allProjects, opts } = input
  const manifestsByPath = getManifestsByPath(allProjects)
  const store = opts.storeControllerAndDir ?? await createStoreController(opts)
  const workspacePackages: WorkspacePackages = arrayOfWorkspacePackagesToMap(allProjects) as WorkspacePackages
  const targetDependenciesField = getSaveType(opts)
  // The workspace manifest writer dedupes against the existing list, so a
  // single drain at the end captures additions across every project.
  const policyHandlers = setupPolicyHandlers(opts)
  const installOpts = extendOptionsForInstall(opts, {
    allProjects,
    manifestsByPath,
    policyHandlers,
    selectedProjectCount: input.selectedProjects.length,
    store,
    targetDependenciesField,
    workspacePackages,
  })
  const getProjectConfig = createProjectConfigGetter(opts)
  const includeDirect = opts.includeDirect ?? {
    dependencies: true,
    devDependencies: true,
    optionalDependencies: true,
  }
  const { params, updateMatch } = createUpdateMatch(input)
  return {
    allProjects,
    cmdFullName: input.cmdFullName,
    params,
    userNamedDeps: input.params.length > 0,
    opts,
    installOpts,
    manifestsByPath,
    store,
    workspacePackages,
    targetDependenciesField,
    policyHandlers,
    getProjectConfig,
    updateToLatest: opts.update && opts.latest,
    includeDirect,
    updateMatch: failOnIndirectVersionedUpdates(input, { includeDirect, params, updateMatch }),
  }
}

interface InstallOptionsExtension {
  allProjects: Project[]
  manifestsByPath: ManifestsByPath
  policyHandlers: PolicyHandlers
  selectedProjectCount: number
  store: StoreControllerAndDir
  targetDependenciesField: DependenciesField | undefined
  workspacePackages: WorkspacePackages
}

function extendOptionsForInstall (opts: RecursiveOptions, extension: InstallOptionsExtension): InstallOptions {
  const projectDependencies = opts.sort !== false
    ? projectsDependencies(opts.allProjectsGraph)
    : new Map((Object.keys(opts.allProjectsGraph) as ProjectRootDir[]).sort().map((rootDir) => [rootDir, []]))
  return Object.assign(opts, {
    allProjects: getAllProjects(extension.manifestsByPath, opts.allProjectsGraph),
    linkWorkspacePackagesDepth: getLinkWorkspacePackagesDepth(opts.linkWorkspacePackages),
    ownLifecycleHooksStdio: 'pipe',
    peer: opts.savePeer,
    pruneLockfileImporters: opts.pruneLockfileImporters ??
      (((opts.ignoredPackages == null) || opts.ignoredPackages.size === 0) &&
        extension.selectedProjectCount === extension.allProjects.length),
    saveCatalogName: opts.saveCatalogName,
    skipRuntimes: opts.runtime === false,
    storeController: extension.store.ctrl,
    storeDir: extension.store.dir,
    targetDependenciesField: extension.targetDependenciesField,
    resolutionVerifiers: extension.store.resolutionVerifiers,
    projectDependencies,
    workspacePackages: extension.workspacePackages,
    handleResolutionPolicyViolations: extension.policyHandlers?.handleResolutionPolicyViolations,
  }) as InstallOptions
}

function getLinkWorkspacePackagesDepth (linkWorkspacePackages: RecursiveOptions['linkWorkspacePackages']): number {
  if (linkWorkspacePackages === 'deep') return Infinity
  return linkWorkspacePackages ? 0 : -1
}

function createProjectConfigGetter (opts: RecursiveOptions): RecursiveContext['getProjectConfig'] {
  const projectConfigRecord = createProjectConfigRecord(opts)
  return projectConfigRecord
    ? manifest => manifest.name ? projectConfigRecord[manifest.name] : undefined
    : () => undefined
}

function createUpdateMatch ({ params, opts, cmdFullName }: RecursiveInput): Pick<RecursiveContext, 'params' | 'updateMatch'> {
  if (cmdFullName !== 'update') return { params, updateMatch: null }
  if (params.length === 0) {
    const ignoreDeps = opts.updateConfig?.ignoreDependencies
    if (ignoreDeps?.length) {
      params = makeIgnorePatterns(ignoreDeps)
    }
  }
  return { params, updateMatch: params.length ? createMatcher(params) : null }
}

/**
 * At `--depth 0` a selector that matches no direct dependency is already
 * `NO_PACKAGE_IN_DEPENDENCIES`; only a deeper update reaches the transitive
 * copy whose version cannot be recorded. `--latest` rejects every versioned
 * selector on its own, direct or not, and has to report that first.
 */
function failOnIndirectVersionedUpdates (
  { opts, selectedProjects }: RecursiveInput,
  { includeDirect, params, updateMatch }: Pick<RecursiveContext, 'includeDirect' | 'params' | 'updateMatch'>
): UpdateDepsMatcher | null {
  if (updateMatch != null && !opts.latest && (opts.depth ?? Infinity) > 0) {
    failOnVersionsOfIndirectUpdateSpecs(params, selectedProjects.map(({ manifest }) => manifest), includeDirect)
  }
  return updateMatch
}

/**
 * The dependencies of a project that an update selects: the ones its update
 * selectors match, or else the selectors as the user passed them.
 */
export function selectUpdateTargets (ctx: RecursiveContext, manifest: ProjectManifest): string[] {
  if (ctx.updateMatch == null) return [...ctx.params]
  return matchDependencies(ctx.updateMatch, manifest, ctx.includeDirect)
}

/**
 * Expands `--latest` without selectors to every direct dependency, and
 * rewrites the selectors to workspace specs under `--workspace`.
 */
export function completeDependencySelectors (
  ctx: RecursiveContext,
  manifest: ProjectManifest,
  selectors: string[]
): string[] {
  if (ctx.updateToLatest && ctx.params.length === 0) {
    selectors = Object.keys(filterDependenciesByType(manifest, ctx.includeDirect))
  }
  if (!ctx.opts.workspace) return selectors
  return toWorkspaceSpecs(selectors, {
    manifest,
    include: ctx.includeDirect,
    workspacePackages: ctx.workspacePackages,
    userNamedDeps: ctx.userNamedDeps,
    fromInteractiveUpdate: ctx.opts.interactiveUpdate,
  })
}

export function getProjectRangeSpecStyle (opts: RecursiveOptions, localConfig: ProjectConfig): RangeSpecStyle {
  return getRangeSpecStyle({
    saveExact: typeof localConfig.saveExact === 'boolean' ? localConfig.saveExact : opts.saveExact,
    savePrefix: typeof localConfig.savePrefix === 'string' ? localConfig.savePrefix : opts.savePrefix,
  })
}

export function createNoPackageInDependenciesError (): PnpmError {
  return new PnpmError('NO_PACKAGE_IN_DEPENDENCIES',
    'None of the specified packages were found in the dependencies of any of the projects.')
}

function getAllProjects (manifestsByPath: ManifestsByPath, allProjectsGraph: ProjectsGraph): ProjectOptions[] {
  return (Object.keys(allProjectsGraph) as ProjectRootDir[]).map((rootDir) => {
    const { rootDirRealPath, modulesDir } = allProjectsGraph[rootDir].package
    return {
      buildIndex: 0,
      manifest: manifestsByPath[rootDir].manifest,
      rootDir,
      rootDirRealPath,
      modulesDir,
    }
  })
}

function getManifestsByPath (projects: Project[]): ManifestsByPath {
  const manifestsByPath: ManifestsByPath = {}
  for (const { rootDir, manifest, writeProjectManifest } of projects) {
    manifestsByPath[rootDir] = { manifest, writeProjectManifest }
  }
  return manifestsByPath
}
