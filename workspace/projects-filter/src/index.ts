import { createMatcher } from '@pnpm/config.matcher'
import type { ProjectRootDir, SupportedArchitectures } from '@pnpm/types'
import { createPkgGraph, type Package, type PackageNode } from '@pnpm/workspace.projects-graph'
import { findWorkspaceProjects, type Project } from '@pnpm/workspace.projects-reader'
import { isSubdir } from 'is-subdir'
import * as micromatch from 'micromatch'
import { difference, partition, pick } from 'ramda'

import { filterProjectsBySelectorObjectsFromDir } from './filterProjectsFromDir.js'
import { getChangedProjects } from './getChangedProjects.js'
import { parseProjectSelector, type ProjectSelector } from './parseProjectSelector.js'

export { filterProjectsBySelectorObjectsFromDir, parseProjectSelector, type ProjectSelector }

export interface WorkspaceFilter {
  filter: string
  followProdDepsOnly: boolean
}

export interface PackageGraph<Pkg extends Package> {
  [id: ProjectRootDir]: PackageNode<Pkg>
}

interface Graph {
  [nodeId: ProjectRootDir]: ProjectRootDir[]
}

interface FilteredGraph<Pkg extends Package> {
  selectedProjectsGraph: PackageGraph<Pkg>
  unmatchedFilters: string[]
}

export interface ReadProjectsResult {
  allProjects: Project[]
  allProjectsGraph: PackageGraph<Project>
  selectedProjectsGraph: PackageGraph<Project>
}

export interface FilterProjectsOptions {
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
    supportedArchitectures?: SupportedArchitectures
  }
): Promise<FilterProjectsFromDirResult> {
  const allProjects = await findWorkspaceProjects(workspaceDir, {
    engineStrict: opts?.engineStrict,
    patterns: opts.patterns,
    sharedWorkspaceLockfile: opts.sharedWorkspaceLockfile,
    nodeVersion: opts.nodeVersion,
    supportedArchitectures: opts.supportedArchitectures,
  })
  return {
    allProjects,
    ...(await filterProjects(allProjects, filter, opts)),
  }
}

export interface FilterProjectsResult<Pkg extends Package> {
  allProjectsGraph: PackageGraph<Pkg>
  selectedProjectsGraph: PackageGraph<Pkg>
  unmatchedFilters: string[]
}

export async function filterProjects<Pkg extends Package> (
  pkgs: Pkg[],
  filter: WorkspaceFilter[],
  opts: FilterProjectsOptions
): Promise<FilterProjectsResult<Pkg>> {
  const projectSelectors = filter.map(({ filter: f, followProdDepsOnly }) => ({ ...parseProjectSelector(f, opts.prefix), followProdDepsOnly }))

  return filterProjectsBySelectorObjects(pkgs, projectSelectors, opts)
}

export async function filterProjectsBySelectorObjects<Pkg extends Package> (
  pkgs: Pkg[],
  projectSelectors: ProjectSelector[],
  opts: {
    linkWorkspacePackages?: boolean
    workspaceDir: string
    testPattern?: string[]
    changedFilesIgnorePattern?: string[]
    useGlobDirFiltering?: boolean
  }
): Promise<{
    allProjectsGraph: PackageGraph<Pkg>
    selectedProjectsGraph: PackageGraph<Pkg>
    unmatchedFilters: string[]
  }> {
  const [prodProjectSelectors, allProjectSelectors] = partition(({ followProdDepsOnly }) => !!followProdDepsOnly, projectSelectors)

  if ((allProjectSelectors.length > 0) || (prodProjectSelectors.length > 0)) {
    let filteredGraph: FilteredGraph<Pkg> | undefined
    const { graph } = createPkgGraph<Pkg>(pkgs, { linkWorkspacePackages: opts.linkWorkspacePackages })

    if (allProjectSelectors.length > 0) {
      filteredGraph = await filterWorkspaceProjects(graph, allProjectSelectors, {
        workspaceDir: opts.workspaceDir,
        testPattern: opts.testPattern,
        changedFilesIgnorePattern: opts.changedFilesIgnorePattern,
        useGlobDirFiltering: opts.useGlobDirFiltering,
      })
    }

    let prodFilteredGraph: FilteredGraph<Pkg> | undefined

    if (prodProjectSelectors.length > 0) {
      const { graph } = createPkgGraph<Pkg>(pkgs, { ignoreDevDeps: true, linkWorkspacePackages: opts.linkWorkspacePackages })
      prodFilteredGraph = await filterWorkspaceProjects(graph, prodProjectSelectors, {
        workspaceDir: opts.workspaceDir,
        testPattern: opts.testPattern,
        changedFilesIgnorePattern: opts.changedFilesIgnorePattern,
        useGlobDirFiltering: opts.useGlobDirFiltering,
      })
    }

    return {
      allProjectsGraph: graph,
      selectedProjectsGraph: {
        ...prodFilteredGraph?.selectedProjectsGraph,
        ...filteredGraph?.selectedProjectsGraph,
      },
      unmatchedFilters: [
        ...(prodFilteredGraph !== undefined ? prodFilteredGraph.unmatchedFilters : []),
        ...(filteredGraph !== undefined ? filteredGraph.unmatchedFilters : []),
      ],
    }
  } else {
    const { graph } = createPkgGraph<Pkg>(pkgs, { linkWorkspacePackages: opts.linkWorkspacePackages })
    return { allProjectsGraph: graph, selectedProjectsGraph: graph, unmatchedFilters: [] }
  }
}

export async function filterWorkspaceProjects<Pkg extends Package> (
  pkgGraph: PackageGraph<Pkg>,
  projectSelectors: ProjectSelector[],
  opts: {
    workspaceDir: string
    testPattern?: string[]
    changedFilesIgnorePattern?: string[]
    useGlobDirFiltering?: boolean
  }
): Promise<{
    selectedProjectsGraph: PackageGraph<Pkg>
    unmatchedFilters: string[]
  }> {
  const [excludeSelectors, includeSelectors] = partition<ProjectSelector>(
    (selector: ProjectSelector) => selector.exclude === true,
    projectSelectors
  )
  const fg = _filterGraph.bind(null, pkgGraph, opts)
  const include = includeSelectors.length === 0
    ? { selected: Object.keys(pkgGraph), unmatchedFilters: [] }
    : await fg(includeSelectors)
  const exclude = await fg(excludeSelectors)
  return {
    selectedProjectsGraph: pick(
      difference(include.selected, exclude.selected) as ProjectRootDir[],
      pkgGraph
    ),
    unmatchedFilters: [...include.unmatchedFilters, ...exclude.unmatchedFilters],
  }
}

async function _filterGraph<Pkg extends Package> (
  pkgGraph: PackageGraph<Pkg>,
  opts: {
    workspaceDir: string
    testPattern?: string[]
    changedFilesIgnorePattern?: string[]
    useGlobDirFiltering?: boolean
  },
  projectSelectors: ProjectSelector[]
): Promise<{
    selected: ProjectRootDir[]
    unmatchedFilters: string[]
  }> {
  const cherryPickedProjects = [] as ProjectRootDir[]
  const walkedDependencies = new Set<ProjectRootDir>()
  const walkedDependents = new Set<ProjectRootDir>()
  const walkedDependentsDependencies = new Set<ProjectRootDir>()
  const graph = pkgGraphToGraph(pkgGraph)
  const unmatchedFilters = [] as string[]
  let reversedGraph: Graph | undefined
  const matchProjectsByPath = opts.useGlobDirFiltering === true
    ? matchProjectsByGlob
    : matchProjectsByExactPath
  for (const selector of projectSelectors) {
    let entryProjects: ProjectRootDir[] | null = null
    if (selector.diff) {
      let ignoreDependentForProjects: ProjectRootDir[] = []
      // eslint-disable-next-line no-await-in-loop
      ;[entryProjects, ignoreDependentForProjects] = await getChangedProjects(
        Object.keys(pkgGraph) as ProjectRootDir[],
        selector.diff,
        {
          changedFilesIgnorePattern: opts.changedFilesIgnorePattern,
          testPattern: opts.testPattern,
          workspaceDir: selector.parentDir ?? opts.workspaceDir,
        }
      )
      selectEntries({
        ...selector,
        includeDependents: false,
      }, ignoreDependentForProjects)
    } else if (selector.parentDir) {
      entryProjects = matchProjectsByPath(pkgGraph, selector.parentDir)
    }
    if (selector.namePattern) {
      if (entryProjects == null) {
        entryProjects = matchProjects(pkgGraph, selector.namePattern)
      } else {
        entryProjects = matchProjects(pick(entryProjects, pkgGraph), selector.namePattern)
      }
    }

    if (entryProjects == null) {
      throw new Error(`Unsupported project selector: ${JSON.stringify(selector)}`)
    }

    if (entryProjects.length === 0) {
      if (selector.namePattern) {
        unmatchedFilters.push(selector.namePattern)
      }
      if (selector.parentDir) {
        unmatchedFilters.push(selector.parentDir)
      }
    }

    selectEntries(selector, entryProjects)
  }
  const walked = new Set([...walkedDependencies, ...walkedDependents, ...walkedDependentsDependencies])
  cherryPickedProjects.forEach((cherryPickedProject) => walked.add(cherryPickedProject))
  return {
    selected: Array.from(walked),
    unmatchedFilters,
  }

  function selectEntries (selector: ProjectSelector, entryProjects: ProjectRootDir[]) {
    if (selector.includeDependencies) {
      pickSubgraph(graph, entryProjects, walkedDependencies, { includeRoot: !selector.excludeSelf })
    }
    if (selector.includeDependents) {
      if (reversedGraph == null) {
        reversedGraph = reverseGraph(graph)
      }
      pickSubgraph(reversedGraph, entryProjects, walkedDependents, { includeRoot: !selector.excludeSelf })
    }

    if (selector.includeDependencies && selector.includeDependents) {
      pickSubgraph(graph, Array.from(walkedDependents), walkedDependentsDependencies, { includeRoot: false })
    }

    if (!selector.includeDependencies && !selector.includeDependents) {
      cherryPickedProjects.push(...entryProjects)
    }
  }
}

function pkgGraphToGraph<Pkg extends Package> (pkgGraph: PackageGraph<Pkg>): Graph {
  const graph: Graph = {}
  for (const nodeId of Object.keys(pkgGraph) as ProjectRootDir[]) {
    graph[nodeId] = pkgGraph[nodeId].dependencies
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

function matchProjects<Pkg extends Package> (
  graph: PackageGraph<Pkg>,
  pattern: string
): ProjectRootDir[] {
  const match = createMatcher(pattern)
  const matches = (Object.keys(graph) as ProjectRootDir[]).filter((id) => graph[id].package.manifest.name && match(graph[id].package.manifest.name!))
  if (matches.length === 0 && !(pattern[0] === '@') && !pattern.includes('/')) {
    const scopedMatches = matchProjects(graph, `@*/${pattern}`)
    return scopedMatches.length !== 1 ? [] : scopedMatches
  }
  return matches
}

function matchProjectsByExactPath<Pkg extends Package> (
  graph: PackageGraph<Pkg>,
  pathStartsWith: string
): ProjectRootDir[] {
  return (Object.keys(graph) as ProjectRootDir[]).filter((parentDir) => isSubdir(pathStartsWith, parentDir))
}

function matchProjectsByGlob<Pkg extends Package> (
  graph: PackageGraph<Pkg>,
  pathStartsWith: string
): ProjectRootDir[] {
  const format = (str: string) => str.replace(/\/$/, '')
  const formattedFilter = pathStartsWith.replace(/\\/g, '/').replace(/\/$/, '')
  return (Object.keys(graph) as ProjectRootDir[]).filter((parentDir) => micromatch.default.isMatch(parentDir, formattedFilter, { format }))
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
