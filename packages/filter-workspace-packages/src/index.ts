import findWorkspacePackages from '@pnpm/find-workspace-packages'
import matcher from '@pnpm/matcher'
import createPkgGraph, { Package, PackageNode } from 'pkgs-graph'
import isSubdir from 'is-subdir'
import * as R from 'ramda'
import getChangedPkgs from './getChangedPackages'
import parsePackageSelector, { PackageSelector } from './parsePackageSelector'

export { parsePackageSelector, PackageSelector }

export interface WorkspaceFilter {
  filter: string
  followProdDepsOnly: boolean
}

export interface PackageGraph<T> {
  [id: string]: PackageNode<T>
}

interface Graph {
  [nodeId: string]: string[]
}

interface FilteredGraph<T> {
  selectedProjectsGraph: PackageGraph<T>
  unmatchedFilters: string[]
}

export async function readProjects (
  workspaceDir: string,
  pkgSelectors: PackageSelector[],
  opts?: {
    linkWorkspacePackages?: boolean
  }
) {
  const allProjects = await findWorkspacePackages(workspaceDir, {})
  const { selectedProjectsGraph } = await filterPkgsBySelectorObjects(
    allProjects,
    pkgSelectors,
    {
      linkWorkspacePackages: opts?.linkWorkspacePackages,
      workspaceDir,
    }
  )
  return { allProjects, selectedProjectsGraph }
}

export async function filterPackages<T> (
  pkgs: Array<Package & T>,
  filter: WorkspaceFilter[],
  opts: {
    linkWorkspacePackages?: boolean
    prefix: string
    workspaceDir: string
    testPattern?: string[]
  }
): Promise<{
    selectedProjectsGraph: PackageGraph<T>
    unmatchedFilters: string[]
  }> {
  const packageSelectors = filter.map(({ filter: f, followProdDepsOnly }) => ({ ...parsePackageSelector(f, opts.prefix), followProdDepsOnly }))

  return filterPkgsBySelectorObjects(pkgs, packageSelectors, opts)
}

export async function filterPkgsBySelectorObjects<T> (
  pkgs: Array<Package & T>,
  packageSelectors: PackageSelector[],
  opts: {
    linkWorkspacePackages?: boolean
    workspaceDir: string
    testPattern?: string[]
  }
): Promise<{
    selectedProjectsGraph: PackageGraph<T>
    unmatchedFilters: string[]
  }> {
  const [prodPackageSelectors, allPackageSelectors] = R.partition(({ followProdDepsOnly }) => !!followProdDepsOnly, packageSelectors)

  if ((allPackageSelectors.length > 0) || (prodPackageSelectors.length > 0)) {
    let filteredGraph: FilteredGraph<T> | undefined

    if (allPackageSelectors.length > 0) {
      const { graph } = createPkgGraph<T>(pkgs, { linkWorkspacePackages: opts.linkWorkspacePackages })
      filteredGraph = await filterGraph(graph, allPackageSelectors, {
        workspaceDir: opts.workspaceDir,
        testPattern: opts.testPattern,
      })
    }

    let prodFilteredGraph: FilteredGraph<T> | undefined

    if (prodPackageSelectors.length > 0) {
      const { graph } = createPkgGraph<T>(pkgs, { ignoreDevDeps: true, linkWorkspacePackages: opts.linkWorkspacePackages })
      prodFilteredGraph = await filterGraph(graph, prodPackageSelectors, {
        workspaceDir: opts.workspaceDir,
        testPattern: opts.testPattern,
      })
    }

    return Promise.resolve({
      selectedProjectsGraph: {
        ...prodFilteredGraph?.selectedProjectsGraph,
        ...filteredGraph?.selectedProjectsGraph,
      },
      unmatchedFilters: [
        ...(prodFilteredGraph !== undefined ? prodFilteredGraph.unmatchedFilters : []),
        ...(filteredGraph !== undefined ? filteredGraph.unmatchedFilters : []),
      ],
    })
  } else {
    const { graph } = createPkgGraph<T>(pkgs, { linkWorkspacePackages: opts.linkWorkspacePackages })
    return Promise.resolve({ selectedProjectsGraph: graph, unmatchedFilters: [] })
  }
}

export default async function filterGraph<T> (
  pkgGraph: PackageGraph<T>,
  packageSelectors: PackageSelector[],
  opts: {
    workspaceDir: string
    testPattern?: string[]
  }
): Promise<{
    selectedProjectsGraph: PackageGraph<T>
    unmatchedFilters: string[]
  }> {
  const [excludeSelectors, includeSelectors] = R.partition<PackageSelector>(
    (selector: PackageSelector) => selector.exclude === true,
    packageSelectors
  )
  const fg = _filterGraph.bind(null, pkgGraph, opts)
  const include = includeSelectors.length === 0
    ? { selected: Object.keys(pkgGraph), unmatchedFilters: [] }
    : await fg(includeSelectors)
  const exclude = await fg(excludeSelectors)
  return {
    selectedProjectsGraph: R.pick(
      R.difference(include.selected, exclude.selected),
      pkgGraph
    ),
    unmatchedFilters: [...include.unmatchedFilters, ...exclude.unmatchedFilters],
  }
}

async function _filterGraph<T> (
  pkgGraph: PackageGraph<T>,
  opts: {
    workspaceDir: string
    testPattern?: string[]
  },
  packageSelectors: PackageSelector[]
): Promise<{
    selected: string[]
    unmatchedFilters: string[]
  }> {
  const cherryPickedPackages = [] as string[]
  const walkedDependencies = new Set<string>()
  const walkedDependents = new Set<string>()
  const walkedDependentsDependencies = new Set<string>()
  const graph = pkgGraphToGraph(pkgGraph)
  const unmatchedFilters = [] as string[]
  let reversedGraph: Graph | undefined
  for (const selector of packageSelectors) {
    let entryPackages: string[] | null = null
    if (selector.diff) {
      let ignoreDependentForPkgs: string[] = []
        ;[entryPackages, ignoreDependentForPkgs] = await getChangedPkgs(Object.keys(pkgGraph),
        selector.diff, { workspaceDir: selector.parentDir ?? opts.workspaceDir, testPattern: opts.testPattern })
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
        entryPackages = matchPackages(R.pick(entryPackages, pkgGraph), selector.namePattern)
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

  function selectEntries (selector: PackageSelector, entryPackages: string[]) {
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

function pkgGraphToGraph<T> (pkgGraph: PackageGraph<T>): Graph {
  const graph: Graph = {}
  Object.keys(pkgGraph).forEach((nodeId) => {
    graph[nodeId] = pkgGraph[nodeId].dependencies
  })
  return graph
}

function reverseGraph (graph: Graph): Graph {
  const reversedGraph: Graph = {}
  Object.keys(graph).forEach((dependentNodeId) => {
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

function matchPackages<T> (
  graph: PackageGraph<T>,
  pattern: string
) {
  const match = matcher(pattern)
  return Object.keys(graph).filter((id) => graph[id].package.manifest.name && match(graph[id].package.manifest.name!))
}

function matchPackagesByPath<T> (
  graph: PackageGraph<T>,
  pathStartsWith: string
) {
  return Object.keys(graph).filter((parentDir) => isSubdir(pathStartsWith, parentDir))
}

function pickSubgraph (
  graph: Graph,
  nextNodeIds: string[],
  walked: Set<string>,
  opts: {
    includeRoot: boolean
  }
) {
  for (const nextNodeId of nextNodeIds) {
    if (!walked.has(nextNodeId)) {
      if (opts.includeRoot) {
        walked.add(nextNodeId)
      }

      if (graph[nextNodeId]) pickSubgraph(graph, graph[nextNodeId], walked, { includeRoot: true })
    }
  }
}
