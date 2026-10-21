import { lexCompare } from '@pnpm/text.ordinal-comparator'
import semver from 'semver'

import {
  compareDependentsTrees,
  type DependentNode,
  type DependentsTree,
  getDependentNodeImporterId,
  setDependentNodeImporterId,
} from './buildDependentsTree.js'

const dependentNodeRootPaths = new WeakMap<DependentNode, string[]>()

export function getDependentNodeRootPaths (node: DependentNode): string[] | undefined {
  return dependentNodeRootPaths.get(node)
}

export function mergeDependentsTrees (trees: DependentsTree[]): DependentsTree[] {
  const byKey = new Map<string, DependentsTree>()
  for (const [treeIndex, tree] of trees.entries()) {
    const clonedDependents = cloneAndTagDependents(tree.dependents, tree.path, treeIndex)
    const key = `${tree.name}\0${tree.version}\0${tree.peersSuffixHash ?? ''}`
    const existing = byKey.get(key)
    if (existing == null) {
      byKey.set(key, {
        ...tree,
        dependents: clonedDependents,
      })
      continue
    }
    mergeIntoTree(existing, tree, clonedDependents)
  }
  const merged = Array.from(byKey.values())
  merged.sort(compareDependentsTrees)
  return merged
}

function cloneAndTagDependents (
  nodes: DependentNode[],
  rootPath: string | undefined,
  treeIndex: number
): DependentNode[] {
  return nodes.map((node) => {
    const cloned: DependentNode = {
      ...node,
      ...(node.dependents ? { dependents: cloneAndTagDependents(node.dependents, rootPath, treeIndex) } : {}),
    }
    const existingImporterId = getDependentNodeImporterId(node)
    if (existingImporterId != null) {
      setDependentNodeImporterId(cloned, existingImporterId)
    } else if (node.depField != null) {
      setDependentNodeImporterId(cloned, `tree:${treeIndex}`)
    }
    const existingRootPaths = dependentNodeRootPaths.get(node)
    if (existingRootPaths != null) {
      dependentNodeRootPaths.set(cloned, [...existingRootPaths])
    } else if (rootPath != null) {
      dependentNodeRootPaths.set(cloned, [rootPath])
    }
    return cloned
  })
}

function mergeIntoTree (target: DependentsTree, source: DependentsTree, sourceDependents: DependentNode[]): void {
  target.displayName ??= source.displayName
  target.path ??= source.path
  target.searchMessage ??= source.searchMessage
  target.dependents = mergeDependentNodes(target.dependents, sourceDependents)
}

function mergeDependentNodes (target: DependentNode[], source: DependentNode[]): DependentNode[] {
  const byKey = new Map<string, DependentNode>()
  for (const node of [...target, ...source]) {
    const key = dependentNodeKey(node)
    const existing = byKey.get(key)
    if (existing == null) {
      byKey.set(key, cloneDependentNode(node))
    } else {
      mergeIntoDependentNode(existing, node)
    }
  }
  const merged = Array.from(byKey.values())
  merged.sort(compareDependentNodes)
  return merged
}

function dependentNodeKey (node: DependentNode): string {
  return `${node.name}\0${node.version}\0${node.peersSuffixHash ?? ''}\0${node.depField ?? ''}\0${node.circular ? '1' : '0'}\0${getDependentNodeImporterId(node) ?? ''}`
}

function cloneDependentNode (node: DependentNode): DependentNode {
  const cloned: DependentNode = {
    ...node,
    ...(node.dependents ? { dependents: [...node.dependents] } : {}),
  }
  const importerId = getDependentNodeImporterId(node)
  if (importerId != null) {
    setDependentNodeImporterId(cloned, importerId)
  }
  const rootPaths = dependentNodeRootPaths.get(node)
  if (rootPaths != null) {
    dependentNodeRootPaths.set(cloned, [...rootPaths])
  }
  return cloned
}

function mergeIntoDependentNode (target: DependentNode, source: DependentNode): void {
  target.displayName ??= source.displayName
  const sourcePaths = dependentNodeRootPaths.get(source)
  if (sourcePaths != null) {
    const targetPaths = dependentNodeRootPaths.get(target) ?? []
    for (const rootPath of sourcePaths) {
      if (!targetPaths.includes(rootPath)) {
        targetPaths.push(rootPath)
      }
    }
    dependentNodeRootPaths.set(target, targetPaths)
  }
  if (target.dependents == null && source.dependents == null) return
  const combined = mergeDependentNodes(target.dependents ?? [], source.dependents ?? [])
  target.dependents = combined.length > 0 ? combined : undefined
  delete target.deduped
}

function compareDependentNodes (left: DependentNode, right: DependentNode): number {
  const nameCmp = lexCompare(left.name, right.name)
  if (nameCmp !== 0) return nameCmp
  const versionCmp = semver.valid(left.version) && semver.valid(right.version)
    ? semver.compare(left.version, right.version)
    : lexCompare(left.version, right.version)
  if (versionCmp !== 0) return versionCmp
  const peerCmp = lexCompare(left.peersSuffixHash ?? '', right.peersSuffixHash ?? '')
  if (peerCmp !== 0) return peerCmp
  const depFieldCmp = lexCompare(left.depField ?? '', right.depField ?? '')
  if (depFieldCmp !== 0) return depFieldCmp
  return lexCompare(getDependentNodeImporterId(left) ?? '', getDependentNodeImporterId(right) ?? '')
}
