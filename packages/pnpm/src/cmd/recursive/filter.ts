import minimatch = require('minimatch')
import {PackageNode} from 'pkgs-graph'
import R = require('ramda')

interface PackageGraph {
  [id: string]: PackageNode,
}

interface Graph {
  [nodeId: string]: string[],
}

export function filterGraphByEntryDirectory (
  pkgGraph: PackageGraph,
  entryDirectory: string,
): PackageGraph {
  if (!pkgGraph[entryDirectory]) return {}

  const walkedDependencies = new Set<string>()
  const graph = pkgGraphToGraph(pkgGraph)
  pickSubgraph(graph, [entryDirectory], walkedDependencies)

  return R.pick(Array.from(walkedDependencies), pkgGraph)
}

export function filterGraph (
  pkgGraph: PackageGraph,
  filters: string[],
): PackageGraph {
  const cherryPickedPackages = [] as string[]
  const walkedDependencies = new Set<string>()
  const walkedDependents = new Set<string>()
  const graph = pkgGraphToGraph(pkgGraph)
  let reversedGraph: Graph | undefined
  for (const filter of filters) {
    if (filter.endsWith('...')) {
      const rootPackagesFilter = filter.substring(0, filter.length - 3)
      const rootPackages = matchPackages(pkgGraph, rootPackagesFilter)
      pickSubgraph(graph, rootPackages, walkedDependencies)
    } else if (filter.startsWith('...')) {
      const leafPackagesFilter = filter.substring(3)
      const leafPackages = matchPackages(pkgGraph, leafPackagesFilter)
      if (!reversedGraph) {
        reversedGraph = reverseGraph(graph)
      }
      pickSubgraph(reversedGraph, leafPackages, walkedDependents)
    } else {
      Array.prototype.push.apply(cherryPickedPackages, matchPackages(pkgGraph, filter))
    }
  }
  const walked = new Set([...walkedDependencies, ...walkedDependents])
  cherryPickedPackages.forEach((cherryPickedPackage) => walked.add(cherryPickedPackage))
  return R.pick(Array.from(walked), pkgGraph)
}

function pkgGraphToGraph (pkgGraph: PackageGraph): Graph {
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

function matchPackages (
  graph: PackageGraph,
  pattern: string,
) {
  return R.keys(graph).filter((id) => graph[id].manifest.name && minimatch(graph[id].manifest.name, pattern))
}

export function filterGraphByScope (
  graph: PackageGraph,
  scope: string,
): PackageGraph {
  const root = matchPackages(graph, scope)
  if (!root.length) return {}

  const subgraphNodeIds = new Set()
  pickSubPkgGraph(graph, root, subgraphNodeIds)

  return R.pick(Array.from(subgraphNodeIds), graph)
}

function pickSubPkgGraph (
  graph: PackageGraph,
  nextNodeIds: string[],
  walked: Set<string>,
) {
  for (const nextNodeId of nextNodeIds) {
    if (!walked.has(nextNodeId)) {
      walked.add(nextNodeId)
      pickSubPkgGraph(graph, graph[nextNodeId].dependencies, walked)
    }
  }
}

function pickSubgraph (
  graph: Graph,
  nextNodeIds: string[],
  walked: Set<string>,
) {
  for (const nextNodeId of nextNodeIds) {
    if (!walked.has(nextNodeId)) {
      walked.add(nextNodeId)
      if (graph[nextNodeId]) pickSubgraph(graph, graph[nextNodeId], walked)
    }
  }
}
