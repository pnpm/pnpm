import type { Catalogs } from '@pnpm/catalogs.types'
import type { ProjectRootDir, SupportedArchitectures } from '@pnpm/types'
import { type BaseProject, createProjectsGraph, type ProjectGraphNode } from '@pnpm/workspace.projects-graph'
import { findWorkspaceProjects, type Project } from '@pnpm/workspace.projects-reader'
import { partition, pick } from 'ramda'

import { filterProjectsBySelectorObjectsFromDir } from './filterProjectsFromDir.js'
import { getChangedProjects } from './getChangedProjects.js'
import { matchProjects, matchProjectsByExactPath, matchProjectsByGlob } from './matchProjects.js'
import { parseProjectSelector, type ProjectSelector } from './parseProjectSelector.js'

export { filterProjectsBySelectorObjectsFromDir, parseProjectSelector, type ProjectSelector }
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

interface FilteredGraph<Pkg extends BaseProject> {
  selectedProjectsGraph: ProjectGraph<Pkg>
  unmatchedFilters: string[]
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

interface FilterWorkspaceProjectsOptions {
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
  const [prodProjectSelectors, allProjectSelectors] = partition(({ followProdDepsOnly }) => !!followProdDepsOnly, projectSelectors)

  if ((allProjectSelectors.length === 0) && (prodProjectSelectors.length === 0)) {
    const { graph } = createProjectsGraph<Pkg>(projects, { catalogs: opts.catalogs, linkWorkspacePackages: opts.linkWorkspacePackages })
    return { allProjectsGraph: graph, selectedProjectsGraph: graph, unmatchedFilters: [] }
  }
  const { graph } = createProjectsGraph<Pkg>(projects, { catalogs: opts.catalogs, linkWorkspacePackages: opts.linkWorkspacePackages })

  const filteredGraph = allProjectSelectors.length > 0
    ? await filterWorkspaceProjects(graph, allProjectSelectors, toFilterWorkspaceProjectsOptions(opts))
    : undefined

  const prod = prodProjectSelectors.length > 0
    ? await filterProdProjects(projects, prodProjectSelectors, opts)
    : undefined

  return {
    allProjectsGraph: graph,
    selectedProjectsGraph: {
      ...prod?.filteredGraph.selectedProjectsGraph,
      ...filteredGraph?.selectedProjectsGraph,
    },
    prodAllProjectsGraph: prod?.graph,
    prodOnlySelectedProjectDirs: prod && getProdOnlySelectedProjectDirs(prod.filteredGraph, filteredGraph),
    unmatchedFilters: [
      ...(prod?.filteredGraph.unmatchedFilters ?? []),
      ...(filteredGraph?.unmatchedFilters ?? []),
    ],
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

async function filterProdProjects<Pkg extends BaseProject> (
  projects: Pkg[],
  prodProjectSelectors: ProjectSelector[],
  opts: FilterWorkspaceProjectsOptions & { catalogs?: Catalogs, linkWorkspacePackages?: boolean }
): Promise<{ graph: ProjectGraph<Pkg>, filteredGraph: FilteredGraph<Pkg> }> {
  const graph = createProjectsGraph<Pkg>(projects, { catalogs: opts.catalogs, ignoreDevDeps: true, linkWorkspacePackages: opts.linkWorkspacePackages }).graph
  const filteredGraph = await filterWorkspaceProjects(graph, prodProjectSelectors, toFilterWorkspaceProjectsOptions(opts))
  return { graph, filteredGraph }
}

function getProdOnlySelectedProjectDirs<Pkg extends BaseProject> (
  prodFilteredGraph: FilteredGraph<Pkg>,
  filteredGraph: FilteredGraph<Pkg> | undefined
): ProjectRootDir[] {
  const regularSelectedProjectDirs = new Set(Object.keys(filteredGraph?.selectedProjectsGraph ?? {}) as ProjectRootDir[])
  return (Object.keys(prodFilteredGraph.selectedProjectsGraph) as ProjectRootDir[])
    .filter((projectDir) => !regularSelectedProjectDirs.has(projectDir))
}

export async function filterWorkspaceProjects<Pkg extends BaseProject> (
  projectsGraph: ProjectGraph<Pkg>,
  projectSelectors: ProjectSelector[],
  opts: FilterWorkspaceProjectsOptions
): Promise<{
  selectedProjectsGraph: ProjectGraph<Pkg>
  unmatchedFilters: string[]
}> {
  if (projectSelectors.length === 0) {
    return {
      selectedProjectsGraph: projectsGraph,
      unmatchedFilters: [],
    }
  }

  const chunks = groupSelectorsIntoChunks(projectSelectors)

  const selectedDirs = new Set<ProjectRootDir>(
    chunks[0].exclude ? (Object.keys(projectsGraph) as ProjectRootDir[]) : []
  )
  const unmatchedFilters: string[] = []

  for (const chunk of chunks) {
    // eslint-disable-next-line no-await-in-loop -- include and exclude chunks apply in order, so each result depends on the ones before it
    const result = await _filterGraph(projectsGraph, opts, chunk.selectors)
    unmatchedFilters.push(...result.unmatchedFilters)
    applyChunkResult(selectedDirs, chunk.exclude, result.selected)
  }

  const validDirs = Array.from(selectedDirs).filter((dir) => projectsGraph[dir] != null)

  return {
    selectedProjectsGraph: pick(validDirs, projectsGraph),
    unmatchedFilters,
  }
}

interface SelectorChunk {
  exclude: boolean
  selectors: ProjectSelector[]
}

function groupSelectorsIntoChunks (projectSelectors: ProjectSelector[]): SelectorChunk[] {
  const chunks: SelectorChunk[] = []
  for (const selector of projectSelectors) {
    const last = chunks[chunks.length - 1]
    if (last && last.exclude === Boolean(selector.exclude)) {
      last.selectors.push(selector)
    } else {
      chunks.push({
        exclude: Boolean(selector.exclude),
        selectors: [selector],
      })
    }
  }
  return chunks
}

function applyChunkResult (selectedDirs: Set<ProjectRootDir>, exclude: boolean, chunkSelectedDirs: ProjectRootDir[]): void {
  if (exclude) {
    for (const dir of chunkSelectedDirs) {
      selectedDirs.delete(dir)
    }
  } else {
    for (const dir of chunkSelectedDirs) {
      selectedDirs.add(dir)
    }
  }
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
