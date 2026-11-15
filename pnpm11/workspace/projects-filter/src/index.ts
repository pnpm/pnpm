import type { Catalogs } from '@pnpm/catalogs.types'
import type { ProjectRootDir, SupportedArchitectures } from '@pnpm/types'
import { type BaseProject, createProjectsGraph, type ProjectGraphNode } from '@pnpm/workspace.projects-graph'
import { findWorkspaceProjects, type Project } from '@pnpm/workspace.projects-reader'
import { pick } from 'ramda'

import { filterProjectsBySelectorObjectsFromDir } from './filterProjectsFromDir.js'
import { type FilteredGraphWithProdResult, runFilterWithProdGraph } from './filterWithProdGraph.js'
import { getChangedProjects } from './getChangedProjects.js'
import { matchProjects, matchProjectsByExactPath, matchProjectsByGlob } from './matchProjects.js'
import { orderWorkspaceFilters, type OrderWorkspaceFiltersOptions } from './orderWorkspaceFilters.js'
import { parseProjectSelector, type ProjectSelector, rebaseProjectSelector } from './parseProjectSelector.js'

export {
  type FilteredGraphWithProdResult,
  filterProjectsBySelectorObjectsFromDir,
  orderWorkspaceFilters,
  type OrderWorkspaceFiltersOptions,
  parseProjectSelector,
  type ProjectSelector,
  rebaseProjectSelector,
}
export { getChangedProjects } from './getChangedProjects.js'

export interface WorkspaceFilter {
  filter: string
  followProdDepsOnly: boolean
  /**
   * Overrides how a `{<dir>}` selector matches, for a selector pnpm
   * generates rather than the user writing it. Left out — every filter a
   * user passes — the selector follows the mode the whole pass runs in,
   * which `legacyDirFiltering` chooses.
   */
  useGlobDirFiltering?: boolean
}

export interface ProjectGraph<Pkg extends BaseProject> {
  [id: ProjectRootDir]: ProjectGraphNode<Pkg>
}

interface Graph {
  [nodeId: ProjectRootDir]: ProjectRootDir[]
}

export interface ReadProjectsResult {
  allProjects: Project[]
  allProjectsGraph: ProjectGraph<Project>
  selectedProjectsGraph: ProjectGraph<Project>
  prodAllProjectsGraph?: ProjectGraph<Project>
  prodOnlySelectedProjectDirs?: ProjectRootDir[]
}

export interface FilterProjectsOptions {
  catalogs?: Catalogs
  linkWorkspacePackages?: boolean
  prefix: string
  workspaceDir: string
  testPattern?: string[]
  changedFilesIgnorePattern?: string[]
  useGlobDirFiltering?: boolean
  sharedWorkspaceLockfile?: boolean
}

export interface FilterProjectsFromDirResult extends FilterProjectsResult<Project> {
  allProjects: Project[]
}

export async function filterProjectsFromDir (
  workspaceDir: string,
  filter: WorkspaceFilter[],
  opts: FilterProjectsOptions & {
    engineStrict?: boolean
    nodeVersion?: string
    nodeVersionFromEnginesRuntime?: boolean
    runtimeOnFail?: 'download' | 'error' | 'warn' | 'ignore'
    skipRuntimes?: boolean
    patterns?: string[]
    modulesDir?: string
    modulesDirsByProjectName?: Record<string, string>
    supportedArchitectures?: SupportedArchitectures
  }
): Promise<FilterProjectsFromDirResult> {
  const allProjects = await findWorkspaceProjects(workspaceDir, {
    engineStrict: opts?.engineStrict,
    patterns: opts.patterns,
    modulesDir: opts.modulesDir,
    modulesDirsByProjectName: opts.modulesDirsByProjectName,
    sharedWorkspaceLockfile: opts.sharedWorkspaceLockfile,
    nodeVersion: opts.nodeVersion,
    nodeVersionFromEnginesRuntime: opts.nodeVersionFromEnginesRuntime,
    runtimeOnFail: opts.runtimeOnFail,
    skipRuntimes: opts.skipRuntimes,
    supportedArchitectures: opts.supportedArchitectures,
  })
  return {
    allProjects,
    ...(await filterProjects(allProjects, filter, opts)),
  }
}

export interface FilterProjectsResult<Pkg extends BaseProject> {
  allProjectsGraph: ProjectGraph<Pkg>
  selectedProjectsGraph: ProjectGraph<Pkg>
  /**
   * The prod-pruned full graph, set when prod-only filters (`--filter-prod`) are
   * used. The recursive-command sorter resolves prod-only selected projects
   * through it so transitive prod dependencies are honored without reintroducing
   * the dev edges the filter dropped.
   */
  prodAllProjectsGraph?: ProjectGraph<Pkg>
  prodOnlySelectedProjectDirs?: ProjectRootDir[]
  unmatchedFilters: string[]
}

export async function filterProjects<Pkg extends BaseProject> (
  projects: Pkg[],
  filter: WorkspaceFilter[],
  opts: FilterProjectsOptions
): Promise<FilterProjectsResult<Pkg>> {
  const projectSelectors = filter.map(({ filter: f, followProdDepsOnly, useGlobDirFiltering }) => ({ ...parseProjectSelector(f, opts.prefix), followProdDepsOnly, useGlobDirFiltering }))

  return filterProjectsBySelectorObjects(projects, projectSelectors, opts)
}

export interface FilterWorkspaceProjectsOptions {
  workspaceDir: string
  testPattern?: string[]
  changedFilesIgnorePattern?: string[]
  useGlobDirFiltering?: boolean
}

export async function filterProjectsBySelectorObjects<Pkg extends BaseProject> (
  projects: Pkg[],
  projectSelectors: ProjectSelector[],
  opts: {
    catalogs?: Catalogs
    linkWorkspacePackages?: boolean
    workspaceDir: string
    testPattern?: string[]
    changedFilesIgnorePattern?: string[]
    useGlobDirFiltering?: boolean
  }
): Promise<{
  allProjectsGraph: ProjectGraph<Pkg>
  selectedProjectsGraph: ProjectGraph<Pkg>
  prodAllProjectsGraph?: ProjectGraph<Pkg>
  prodOnlySelectedProjectDirs?: ProjectRootDir[]
  unmatchedFilters: string[]
}> {
  const { graph } = createProjectsGraph<Pkg>(projects, { catalogs: opts.catalogs, linkWorkspacePackages: opts.linkWorkspacePackages })
  if (projectSelectors.length === 0) {
    return { allProjectsGraph: graph, selectedProjectsGraph: graph, unmatchedFilters: [] }
  }

  const hasProdSelectors = projectSelectors.some(({ followProdDepsOnly }) => Boolean(followProdDepsOnly))
  const prodAllProjectsGraph = hasProdSelectors
    ? createProjectsGraph<Pkg>(projects, { catalogs: opts.catalogs, ignoreDevDeps: true, linkWorkspacePackages: opts.linkWorkspacePackages }).graph
    : undefined
  const filtered = await filterWorkspaceProjectsWithProdGraph(
    { allProjectsGraph: graph, prodProjectsGraph: prodAllProjectsGraph },
    projectSelectors,
    toFilterWorkspaceProjectsOptions(opts)
  )

  return {
    allProjectsGraph: graph,
    selectedProjectsGraph: filtered.selectedProjectsGraph,
    prodAllProjectsGraph,
    prodOnlySelectedProjectDirs: filtered.prodOnlySelectedProjectDirs,
    unmatchedFilters: filtered.unmatchedFilters,
  }
}

function toFilterWorkspaceProjectsOptions (opts: FilterWorkspaceProjectsOptions): FilterWorkspaceProjectsOptions {
  return {
    workspaceDir: opts.workspaceDir,
    testPattern: opts.testPattern,
    changedFilesIgnorePattern: opts.changedFilesIgnorePattern,
    useGlobDirFiltering: opts.useGlobDirFiltering,
  }
}

export async function filterWorkspaceProjects<Pkg extends BaseProject> (
  projectsGraph: ProjectGraph<Pkg>,
  projectSelectors: ProjectSelector[],
  opts: FilterWorkspaceProjectsOptions
): Promise<{
  selectedProjectsGraph: ProjectGraph<Pkg>
  unmatchedFilters: string[]
}> {
  const { selectedProjectsGraph, unmatchedFilters } = await filterWorkspaceProjectsWithProdGraph(
    { allProjectsGraph: projectsGraph },
    projectSelectors,
    opts
  )
  return { selectedProjectsGraph, unmatchedFilters }
}

export async function filterWorkspaceProjectsWithProdGraph<Pkg extends BaseProject> (
  graphs: {
    allProjectsGraph: ProjectGraph<Pkg>
    prodProjectsGraph?: ProjectGraph<Pkg>
  },
  projectSelectors: ProjectSelector[],
  opts: FilterWorkspaceProjectsOptions
): Promise<FilteredGraphWithProdResult<Pkg>> {
  return runFilterWithProdGraph({ ...graphs, filterGraph: _filterGraph }, projectSelectors, opts)
}

interface Selection {
  graph: Graph
  reversedGraph?: Graph
  cherryPickedProjects: ProjectRootDir[]
  walkedDependencies: Set<ProjectRootDir>
  walkedDependents: Set<ProjectRootDir>
  walkedDependentsDependencies: Set<ProjectRootDir>
}

async function _filterGraph<Pkg extends BaseProject> (
  projectsGraph: ProjectGraph<Pkg>,
  opts: FilterWorkspaceProjectsOptions,
  projectSelectors: ProjectSelector[]
): Promise<{
  selected: ProjectRootDir[]
  unmatchedFilters: string[]
}> {
  const selection: Selection = {
    cherryPickedProjects: [],
    walkedDependencies: new Set(),
    walkedDependents: new Set(),
    walkedDependentsDependencies: new Set(),
    graph: projectsGraphToGraph(projectsGraph),
  }
  const unmatchedFilters = [] as string[]
  for (const selector of projectSelectors) {
    // eslint-disable-next-line no-await-in-loop -- each selector adds to the shared walked sets, which selectEntries mutates
    const entryProjects = await findEntryProjects({ projectsGraph, opts, selector, selection })

    if (entryProjects.length === 0) {
      unmatchedFilters.push(...getUnmatchedFilters(selector))
    }

    selectEntries(selection, selector, entryProjects)
  }
  return {
    selected: collectSelectedProjects(selection),
    unmatchedFilters,
  }
}

interface FindEntryProjectsParams<Pkg extends BaseProject> {
  projectsGraph: ProjectGraph<Pkg>
  opts: FilterWorkspaceProjectsOptions
  selector: ProjectSelector
  selection: Selection
}

async function findEntryProjects<Pkg extends BaseProject> (params: FindEntryProjectsParams<Pkg>): Promise<ProjectRootDir[]> {
  const { projectsGraph, selector } = params
  const entryProjects = await findEntryProjectsByLocation(params)
  if (selector.namePattern) {
    if (entryProjects == null) {
      return matchProjects(projectsGraph, selector.namePattern)
    }
    return matchProjects(pick(entryProjects, projectsGraph), selector.namePattern)
  }

  if (entryProjects == null) {
    throw new Error(`Unsupported project selector: ${JSON.stringify(selector)}`)
  }
  return entryProjects
}

async function findEntryProjectsByLocation<Pkg extends BaseProject> (
  { projectsGraph, opts, selector, selection }: FindEntryProjectsParams<Pkg>
): Promise<ProjectRootDir[] | null> {
  if (selector.diff) {
    const [changedProjects, ignoreDependentForProjects] = await getChangedProjects(
      Object.keys(projectsGraph) as ProjectRootDir[],
      selector.diff,
      {
        allProjects: Object.values(projectsGraph).map((node) => node.package),
        changedFilesIgnorePattern: opts.changedFilesIgnorePattern,
        testPattern: opts.testPattern,
        useGlobDirFiltering: selector.useGlobDirFiltering ?? opts.useGlobDirFiltering,
        workingDir: selector.parentDir,
        workspaceDir: opts.workspaceDir,
      }
    )
    selectEntries(selection, {
      ...selector,
      includeDependents: false,
    }, ignoreDependentForProjects)
    return changedProjects
  }
  if (selector.parentDir) {
    const matchProjectsByPath = (selector.useGlobDirFiltering ?? opts.useGlobDirFiltering) === true
      ? matchProjectsByGlob
      : matchProjectsByExactPath
    return matchProjectsByPath(projectsGraph, selector.parentDir)
  }
  return null
}

function getUnmatchedFilters (selector: ProjectSelector): string[] {
  const unmatchedFilters: string[] = []
  if (selector.namePattern) {
    unmatchedFilters.push(selector.namePattern)
  }
  if (selector.parentDir) {
    unmatchedFilters.push(selector.parentDir)
  }
  return unmatchedFilters
}

function selectEntries (selection: Selection, selector: ProjectSelector, entryProjects: ProjectRootDir[]): void {
  if (selector.includeDependencies) {
    pickSubgraph(selection.graph, entryProjects, selection.walkedDependencies, { includeRoot: !selector.excludeSelf })
  }
  if (selector.includeDependents) {
    selection.reversedGraph ??= reverseGraph(selection.graph)
    const selectorDependents = new Set<ProjectRootDir>()
    pickSubgraph(selection.reversedGraph, entryProjects, selectorDependents, { includeRoot: !selector.excludeSelf })
    selectorDependents.forEach((dependent) => selection.walkedDependents.add(dependent))

    if (selector.includeDependencies) {
      pickSubgraph(selection.graph, Array.from(selectorDependents), selection.walkedDependentsDependencies, { includeRoot: false })
    }
  }

  if (!selector.includeDependencies && !selector.includeDependents) {
    selection.cherryPickedProjects.push(...entryProjects)
  }
}

function collectSelectedProjects (selection: Selection): ProjectRootDir[] {
  const walked = new Set([...selection.walkedDependencies, ...selection.walkedDependents, ...selection.walkedDependentsDependencies])
  selection.cherryPickedProjects.forEach((cherryPickedProject) => walked.add(cherryPickedProject))
  return Array.from(walked)
}

function projectsGraphToGraph<Pkg extends BaseProject> (projectsGraph: ProjectGraph<Pkg>): Graph {
  const graph: Graph = {}
  for (const nodeId of Object.keys(projectsGraph) as ProjectRootDir[]) {
    graph[nodeId] = projectsGraph[nodeId].dependencies
  }
  return graph
}

function reverseGraph (graph: Graph): Graph {
  const reversedGraph: Graph = {}
  for (const dependentNodeId of Object.keys(graph) as ProjectRootDir[]) {
    for (const dependencyNodeId of graph[dependentNodeId]) {
      if (!reversedGraph[dependencyNodeId]) {
        reversedGraph[dependencyNodeId] = [dependentNodeId]
      } else {
        reversedGraph[dependencyNodeId].push(dependentNodeId)
      }
    }
  }
  return reversedGraph
}

function pickSubgraph (
  graph: Graph,
  nextNodeIds: ProjectRootDir[],
  walked: Set<ProjectRootDir>,
  opts: {
    includeRoot: boolean
  }
): void {
  for (const nextNodeId of nextNodeIds) {
    if (!walked.has(nextNodeId)) {
      if (opts.includeRoot) {
        walked.add(nextNodeId)
      }

      if (graph[nextNodeId]) pickSubgraph(graph, graph[nextNodeId], walked, { includeRoot: true })
    }
  }
}
