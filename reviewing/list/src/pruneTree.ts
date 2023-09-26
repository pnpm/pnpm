import { type DependenciesHierarchy, type PackageNode } from '@pnpm/reviewing.dependencies-hierarchy'
import { type PackageDependencyHierarchy } from './types'
import { createHash } from 'crypto'

export function pruneTreeToGetFirst10EndLeafs (trees: PackageDependencyHierarchy[] | null): PackageDependencyHierarchy[] {
  if (trees === null) {
    return []
  }

  return trees.map((tree) => {
    const endLeafPaths: PackageNode[][] = []
    const visitedNodes = new Set<string>()

    function dfs (node: PackageNode, path: PackageNode[]): void {
      if (node.circular) {
        return
      }

      const nodeId = `${node.name}@${node.version}`
      if (visitedNodes.has(nodeId)) {
        return
      }

      visitedNodes.add(nodeId)
      const newPath = [...path, node]

      if (!node.dependencies || node.dependencies.length === 0) {
        endLeafPaths.push(newPath)
        if (endLeafPaths.length >= 10) {
          return
        }
      }

      for (const child of node.dependencies ?? []) {
        dfs(child, newPath)
        if (endLeafPaths.length >= 10) {
          return
        }
      }

      visitedNodes.delete(nodeId)
    }

    if (tree.dependencies) {
      for (const node of tree.dependencies) {
        dfs(node, [])
      }
    }

    const first10Paths = endLeafPaths.slice(0, 10)
    const map = new Map<string, PackageNode>()
    const newTree: DependenciesHierarchy = { dependencies: [] }

    for (const path of first10Paths) {
      let currentDependencies: PackageNode[] = newTree.dependencies!
      let pathSoFar = ''

      for (const node of path) {
        pathSoFar += `${node.name}@${node.version},`
        const id = createHash('sha256').update(pathSoFar).digest('hex')
        let existingNode = map.get(id)

        if (!existingNode) {
          existingNode = { ...node, dependencies: [] }
          currentDependencies.push(existingNode)
          map.set(id, existingNode)
        }

        currentDependencies = existingNode.dependencies!
      }
    }

    return {
      ...tree,
      dependencies: newTree.dependencies,
    }
  })
}
