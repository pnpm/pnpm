import { createMatcher } from '@pnpm/matcher'
import { type ProjectRootDir, type SupportedArchitectures } from '@pnpm/types'
import { findWorkspacePackages, type Project } from '@pnpm/workspace.find-packages'
import { createPkgGraph, type Package, type PackageNode } from '@pnpm/workspace.pkgs-graph'
import isSubdir from 'is-subdir'
import difference from 'ramda/src/difference'
import partition from 'ramda/src/partition'
import pick from 'ramda/src/pick'
import * as micromatch from 'micromatch'
import { getChangedPackages } from './getChangedPackages'
import { parsePackageSelector, type PackageSelector } from './parsePackageSelector'

export { parsePackageSelector, type PackageSelector }

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

export interface FilterPackagesOptions {
  linkWorkspacePackages?: boolean
  prefix: string
  workspaceDir: string
  testPattern?: string[]
  changedFilesIgnorePattern?: string[]
  useGlobDirFiltering?: boolean
  sharedWorkspaceLockfile?: boolean
}

export interface FilterPackagesFromDirResult extends FilterPackagesResult<Project> {
  allProjects: Project[]
}

export async function filterPackagesFromDir (
  workspaceDir: string,
  filter: WorkspaceFilter[],
  opts: FilterPackagesOptions & {
    engineStrict?: boolean
    nodeVersion?: string
    patterns?: string[]
    supportedArchitectures?: SupportedArchitectures
  }
): Promise<FilterPackagesFromDirResult> {
  const allProjects = await findWorkspacePackages(workspaceDir, {
    engineStrict: opts?.engineStrict,
    patterns: opts.patterns,
    sharedWorkspaceLockfile: opts.sharedWorkspaceLockfile,
    nodeVersion: opts.nodeVersion,
    supportedArchitectures: opts.supportedArchitectures,
  })
  return {
    allProjects,
    ...(await filterPackages(allProjects, filter, opts)),
  }
}

export interface FilterPackagesResult<Pkg extends Package> {
  allProjectsGraph: PackageGraph<Pkg>
  selectedProjectsGraph: PackageGraph<Pkg>
  unmatchedFilters: string[]
}

export async function filterPackages<Pkg extends Package> (
  pkgs: Pkg[],
  filter: WorkspaceFilter[],
  opts: FilterPackagesOptions
): Promise<FilterPackagesResult<Pkg>> {
  const packageSelectors = filter.map(({ filter: f, followProdDepsOnly }) => ({ ...parsePackageSelector(f, opts.prefix), followProdDepsOnly }))

  return filterPkgsBySelectorObjects(pkgs, packageSelectors, opts)
}

export async function filterPkgsBySelectorObjects<Pkg extends Package> (
  pkgs: Pkg[],
  packageSelectors: PackageSelector[],
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
  const [prodPackageSelectors, allPackageSelectors] = partition(({ followProdDepsOnly }) => !!followProdDepsOnly, packageSelectors)

  if ((allPackageSelectors.length > 0) || (prodPackageSelectors.length > 0)) {
    let filteredGraph: FilteredGraph<Pkg> | undefined
    const { graph } = createPkgGraph<Pkg>(pkgs, { linkWorkspacePackages: opts.linkWorkspacePackages })

    if (allPackageSelectors.length > 0) {
      filteredGraph = await filterWorkspacePackages(graph, allPackageSelectors, {
        workspaceDir: opts.workspaceDir,
        testPattern: opts.testPattern,
        changedFilesIgnorePattern: opts.changedFilesIgnorePattern,
        useGlobDirFiltering: opts.useGlobDirFiltering,
      })
    }

    let prodFilteredGraph: FilteredGraph<Pkg> | undefined

    if (prodPackageSelectors.length > 0) {
      const { graph } = createPkgGraph<Pkg>(pkgs, { ignoreDevDeps: true, linkWorkspacePackages: opts.linkWorkspacePackages })
      prodFilteredGraph = await filterWorkspacePackages(graph, prodPackageSelectors, {
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

export async function filterWorkspacePackages<Pkg extends Package> (
  pkgGraph: PackageGraph<Pkg>,
  packageSelectors: PackageSelector[],
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
  const [excludeSelectors, includeSelectors] = partition<PackageSelector>(
    (selector: PackageSelector) => selector.exclude === true,
    packageSelectors
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
  packageSelectors: PackageSelector[]
): Promise<{
    selected: ProjectRootDir[]
    unmatchedFilters: string[]
  }> {
  const cherryPickedPackages = [] as ProjectRootDir[]
  const walkedDependencies = new Set<ProjectRootDir>()
  const walkedDependents = new Set<ProjectRootDir>()
  const walkedDependentsDependencies = new Set<ProjectRootDir>()
  const graph = pkgGraphToGraph(pkgGraph)
  const unmatchedFilters = [] as string[]
  let reversedGraph: Graph | undefined
  const matchPackagesByPath = opts.useGlobDirFiltering === true
    ? matchPackagesByGlob
    : matchPackagesByExactPath
  for (const selector of packageSelectors) {
    let entryPackages: ProjectRootDir[] | null = null
    if (selector.diff) {
      let ignoreDependentForPkgs: ProjectRootDir[] = []
      // eslint-disable-next-line no-await-in-loop
      ;[entryPackages, ignoreDependentForPkgs] = await getChangedPackages(
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
      }, ignoreDependentForPkgs)
    } else if (selector.parentDir) {
      entryPackages = matchPackagesByPath(pkgGraph, selector.parentDir)
    }
    if (selector.namePattern) {
      if (entryPackages == null) {
        entryPackages = matchPackages(pkgGraph, selector.namePattern)
      } else {
        entryPackages = matchPackages(pick(entryPackages, pkgGraph), selector.namePattern)
      }
    }

    if (entryPackages == null) {
      throw new Error(`Unsupported package selector: ${JSON.stringify(selector)}`)
    }

    if (entryPackages.length === 0) {
      if (selector.namePattern) {
        unmatchedFilters.push(selector.namePattern)
      }
      if (selector.parentDir) {
        unmatchedFilters.push(selector.parentDir)
      }
    }

    selectEntries(selector, entryPackages)
  }
  const walked = new Set([...walkedDependencies, ...walkedDependents, ...walkedDependentsDependencies])
  cherryPickedPackages.forEach((cherryPickedPackage) => walked.add(cherryPickedPackage))
  return {
    selected: Array.from(walked),
    unmatchedFilters,
  }

  function selectEntries (selector: PackageSelector, entryPackages: ProjectRootDir[]) {
    if (selector.includeDependencies) {
      pickSubgraph(graph, entryPackages, walkedDependencies, { includeRoot: !selector.excludeSelf })
    }
    if (selector.includeDependents) {
      if (reversedGraph == null) {
        reversedGraph = reverseGraph(graph)
      }
      pickSubgraph(reversedGraph, entryPackages, walkedDependents, { includeRoot: !selector.excludeSelf })
    }

    if (selector.includeDependencies && selector.includeDependents) {
      pickSubgraph(graph, Array.from(walkedDependents), walkedDependentsDependencies, { includeRoot: false })
    }

    if (!selector.includeDependencies && !selector.includeDependents) {
      Array.prototype.push.apply(cherryPickedPackages, entryPackages)
    }
  }
}

function pkgGraphToGraph<Pkg extends Package> (pkgGraph: PackageGraph<Pkg>): Graph {
  const graph: Graph = {}
  ;(Object.keys(pkgGraph) as ProjectRootDir[]).forEach((nodeId) => {
    graph[nodeId] = pkgGraph[nodeId].dependencies
  })
  return graph
}

function reverseGraph (graph: Graph): Graph {
  const reversedGraph: Graph = {}
  ;(Object.keys(graph) as ProjectRootDir[]).forEach((dependentNodeId) => {
    graph[dependentNodeId].forEach((dependencyNodeId) => {
      if (!reversedGraph[dependencyNodeId]) {
        reversedGraph[dependencyNodeId] = [dependentNodeId]
      } else {
        reversedGraph[dependencyNodeId].push(dependentNodeId)
      }
    })
  })
  return reversedGraph
}

function matchPackages<Pkg extends Package> (
  graph: PackageGraph<Pkg>,
  pattern: string
): ProjectRootDir[] {
  const match = createMatcher(pattern)
  const matches = (Object.keys(graph) as ProjectRootDir[]).filter((id) => graph[id].package.manifest.name && match(graph[id].package.manifest.name!))
  if (matches.length === 0 && !(pattern[0] === '@') && !pattern.includes('/')) {
    const scopedMatches = matchPackages(graph, `@*/${pattern}`)
    return scopedMatches.length !== 1 ? [] : scopedMatches
  }
  return matches
}

function matchPackagesByExactPath<Pkg extends Package> (
  graph: PackageGraph<Pkg>,
  pathStartsWith: string
): ProjectRootDir[] {
  return (Object.keys(graph) as ProjectRootDir[]).filter((parentDir) => isSubdir(pathStartsWith, parentDir))
}

function matchPackagesByGlob<Pkg extends Package> (
  graph: PackageGraph<Pkg>,
  pathStartsWith: string
): ProjectRootDir[] {
  const format = (str: string) => str.replace(/\/$/, '')
  const formattedFilter = pathStartsWith.replace(/\\/g, '/').replace(/\/$/, '')
  return (Object.keys(graph) as ProjectRootDir[]).filter((parentDir) => micromatch.isMatch(parentDir, formattedFilter, { format }))
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
