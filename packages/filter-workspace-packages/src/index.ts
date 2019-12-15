import matcher from '@pnpm/matcher'
import isSubdir = require('is-subdir')
import { PackageNode } from 'pkgs-graph'
import R = require('ramda')
import parsePackageSelector, { PackageSelector } from './parsePackageSelector'

export { parsePackageSelector, PackageSelector }

export interface PackageGraph<T> {
  [id: string]: PackageNode<T>,
}

interface Graph {
  [nodeId: string]: string[],
}

export default function filterGraph<T> (
  pkgGraph: PackageGraph<T>,
  packageSelectors: PackageSelector[],
): PackageGraph<T> {
  const cherryPickedPackages = [] as string[]
  const walkedDependencies = new Set<string>()
  const walkedDependents = new Set<string>()
  const graph = pkgGraphToGraph(pkgGraph)
  let reversedGraph: Graph | undefined
  for (const { excludeSelf, pattern, scope, selectBy } of packageSelectors) {
    const entryPackages = selectBy === 'name'
      ? matchPackages(pkgGraph, pattern)
      : matchPackagesByPath(pkgGraph, pattern)

    switch (scope) {
      case 'dependencies':
        pickSubgraph(graph, entryPackages, walkedDependencies, { includeRoot: !excludeSelf })
        continue
      case 'dependents':
        if (!reversedGraph) {
          reversedGraph = reverseGraph(graph)
        }
        pickSubgraph(reversedGraph, entryPackages, walkedDependents, { includeRoot: !excludeSelf })
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
  pattern: string,
) {
  const match = matcher(pattern)
  return Object.keys(graph).filter((id) => graph[id].package.manifest.name && match(graph[id].package.manifest.name))
}

function matchPackagesByPath<T> (
  graph: PackageGraph<T>,
  pathStartsWith: string,
) {
  return Object.keys(graph).filter((location) => isSubdir(pathStartsWith, location))
}

function pickSubgraph (
  graph: Graph,
  nextNodeIds: string[],
  walked: Set<string>,
  opts: {
    includeRoot: boolean
  },
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
