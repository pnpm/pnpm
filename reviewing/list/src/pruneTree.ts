import { createHash } from 'crypto'

import type {
  PackageNode,
  DependenciesHierarchy,
} from '@pnpm/reviewing.dependencies-hierarchy'
import type { PackageDependencyHierarchy, PackageInfo } from '@pnpm/types'

export function pruneDependenciesTrees(
  trees: PackageDependencyHierarchy[] | null,
  limit: number
): PackageDependencyHierarchy[] {
  if (trees === null) {
    return []
  }

  return trees.map((tree) => {
    const endLeafPaths: (PackageNode | PackageInfo)[][] = []
    const visitedNodes = new Set<string>()

    function findEndLeaves(node: PackageNode | PackageInfo, path: (PackageNode | PackageInfo)[]): void {
      if (node.circular) {
        return
      }

      const nodeId = `${node.name}@${node.version}`
      if (visitedNodes.has(nodeId)) {
        return
      }

      visitedNodes.add(nodeId)
      const newPath: PackageInfo[] = [...path, node]

      if (!node.dependencies || node.dependencies.length === 0) {
        endLeafPaths.push(newPath)

        if (endLeafPaths.length >= limit) {
          return
        }
      }

      for (const child of node.dependencies ?? []) {
        findEndLeaves(child, newPath)

        if (endLeafPaths.length >= limit) {
          return
        }
      }

      visitedNodes.delete(nodeId)
    }

    if (tree.dependencies) {
      for (const node of tree.dependencies) {
        findEndLeaves(node, [])
      }
    }

    const firstNPaths = endLeafPaths.slice(0, limit)

    const map = new Map<string, PackageNode | PackageInfo>()

    const newTree: DependenciesHierarchy = { dependencies: [] }

    for (const path of firstNPaths) {
      let currentDependencies: (PackageNode | PackageInfo)[] | undefined = newTree.dependencies

      let pathSoFar = ''

      for (const node of path) {
        pathSoFar += `${node.name}@${node.version},`

        const id = createHash('sha256').update(pathSoFar).digest('hex')

        let existingNode = map.get(id)

        if (!existingNode) {
          existingNode = { ...node, dependencies: [] }

          currentDependencies?.push(existingNode)

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
