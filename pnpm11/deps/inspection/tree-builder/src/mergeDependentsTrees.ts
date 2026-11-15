import { lexCompare } from '@pnpm/text.ordinal-comparator'
import semver from 'semver'

import type {
  DependentNode,
  DependentsTree,
} from './buildDependentsTree.js'
import {
  compareDependentsTrees,
  getDependentNodeImporterId,
  getDependentNodeMergeId,
  getDependentsTreeMergeId,
  setDependentNodeImporterId,
  setDependentNodeMergeId,
  setDependentsTreeMergeId,
} from './dependentsTreeIdentity.js'

const dependentNodeRootPaths = new WeakMap<DependentNode, string[]>()

export function getDependentNodeRootPaths (node: DependentNode): string[] | undefined {
  return dependentNodeRootPaths.get(node)
}

export function mergeDependentsTrees (trees: DependentsTree[]): DependentsTree[] {
  const byKey = new Map<string, DependentsTree>()
  for (const [treeIndex, tree] of trees.entries()) {
    const clonedDependents = cloneAndTagDependents(tree.dependents, tree.path, treeIndex)
    const treeMergeId = getDependentsTreeMergeId(tree)
    const key = `${tree.name}\0${tree.version ?? ''}\0${tree.peersSuffixHash ?? ''}\0${treeMergeId ?? ''}`
    const existing = byKey.get(key)
    if (existing == null) {
      const clonedTree: DependentsTree = {
        ...tree,
        dependents: clonedDependents,
      }
      if (treeMergeId != null) {
        setDependentsTreeMergeId(clonedTree, treeMergeId)
      }
      byKey.set(key, clonedTree)
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
    copyClonedDependentMetadata(cloned, node, rootPath, treeIndex)
    return cloned
  })
}

function copyClonedDependentMetadata (
  cloned: DependentNode,
  source: DependentNode,
  rootPath: string | undefined,
  treeIndex: number
): void {
  const existingImporterId = getDependentNodeImporterId(source) ?? (source.depField != null ? `tree:${treeIndex}` : undefined)
  if (existingImporterId != null) {
    setDependentNodeImporterId(cloned, existingImporterId)
  }
  const existingMergeId = getDependentNodeMergeId(source)
  if (existingMergeId != null) {
    setDependentNodeMergeId(cloned, existingMergeId)
  }
  const existingRootPaths = dependentNodeRootPaths.get(source)
  if (existingRootPaths != null) {
    dependentNodeRootPaths.set(cloned, [...existingRootPaths])
  } else if (rootPath != null) {
    dependentNodeRootPaths.set(cloned, [rootPath])
  }
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
  return `${node.name}\0${node.version ?? ''}\0${node.peersSuffixHash ?? ''}\0${getDependentNodeMergeId(node) ?? ''}\0${node.depField ?? ''}\0${node.circular ? '1' : '0'}\0${getDependentNodeImporterId(node) ?? ''}`
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
  const mergeId = getDependentNodeMergeId(node)
  if (mergeId != null) {
    setDependentNodeMergeId(cloned, mergeId)
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
  const leftVersion = left.version ?? ''
  const rightVersion = right.version ?? ''
  const versionCmp = semver.valid(leftVersion) && semver.valid(rightVersion)
    ? semver.compare(leftVersion, rightVersion)
    : lexCompare(leftVersion, rightVersion)
  if (versionCmp !== 0) return versionCmp
  const peerCmp = lexCompare(left.peersSuffixHash ?? '', right.peersSuffixHash ?? '')
  if (peerCmp !== 0) return peerCmp
  const mergeCmp = lexCompare(getDependentNodeMergeId(left) ?? '', getDependentNodeMergeId(right) ?? '')
  if (mergeCmp !== 0) return mergeCmp
  const depFieldCmp = lexCompare(left.depField ?? '', right.depField ?? '')
  if (depFieldCmp !== 0) return depFieldCmp
  return lexCompare(getDependentNodeImporterId(left) ?? '', getDependentNodeImporterId(right) ?? '')
}
