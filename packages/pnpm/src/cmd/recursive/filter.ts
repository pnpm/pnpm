import isSubdir = require('is-subdir')
import minimatch = require('minimatch')
import { PackageNode } from 'pkgs-graph'
import R = require('ramda')
import { PackageSelector } from '../../parsePackageSelectors'

interface PackageGraph {
  [id: string]: PackageNode<{ fileName: string }>,
}

interface Graph {
  [nodeId: string]: string[],
}

export function filterGraph (
  pkgGraph: PackageGraph,
  packageSelectors: PackageSelector[],
): PackageGraph {
  const cherryPickedPackages = [] as string[]
  const walkedDependencies = new Set<string>()
  const walkedDependents = new Set<string>()
  const graph = pkgGraphToGraph(pkgGraph)
  let reversedGraph: Graph | undefined
  for (const selector of packageSelectors) {
    const entryPackages = selector.selectBy === 'name'
      ? matchPackages(pkgGraph, selector.matcher)
      : matchPackagesByPath(pkgGraph, selector.matcher)

    switch (selector.scope) {
      case 'dependencies':
        pickSubgraph(graph, entryPackages, walkedDependencies)
        continue
      case 'dependents':
        if (!reversedGraph) {
          reversedGraph = reverseGraph(graph)
        }
        pickSubgraph(reversedGraph, entryPackages, walkedDependents)
        continue
      case 'exact':
        Array.prototype.push.apply(cherryPickedPackages, entryPackages)
        continue
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
  return R.keys(graph).filter((id) => graph[id].package.manifest.name && minimatch(graph[id].package.manifest.name, pattern))
}

function matchPackagesByPath (
  graph: PackageGraph,
  pathStartsWith: string,
) {
  return R.keys(graph).filter((location) => isSubdir(pathStartsWith, location))
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
