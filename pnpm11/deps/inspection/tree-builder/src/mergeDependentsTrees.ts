import { lexCompare } from '@pnpm/text.ordinal-comparator'
import semver from 'semver'

import { compareDependentsTrees, type DependentNode, type DependentsTree } from './buildDependentsTree.js'

export function mergeDependentsTrees (trees: DependentsTree[]): DependentsTree[] {
  const byKey = new Map<string, DependentsTree>()
  for (const tree of trees) {
    const key = `${tree.name}\0${tree.version}\0${tree.peersSuffixHash ?? ''}`
    const existing = byKey.get(key)
    if (existing == null) {
      byKey.set(key, {
        ...tree,
        dependents: [...tree.dependents],
      })
      continue
    }
    mergeIntoTree(existing, tree)
  }
  const merged = Array.from(byKey.values())
  merged.sort(compareDependentsTrees)
  return merged
}

function mergeIntoTree (target: DependentsTree, source: DependentsTree): void {
  target.displayName ??= source.displayName
  target.path ??= source.path
  target.searchMessage ??= source.searchMessage
  target.dependents = mergeDependentNodes(target.dependents, source.dependents)
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
  return `${node.name}\0${node.version}\0${node.peersSuffixHash ?? ''}\0${node.depField ?? ''}\0${node.circular ? '1' : '0'}`
}

function cloneDependentNode (node: DependentNode): DependentNode {
  return {
    ...node,
    ...(node.dependents ? { dependents: [...node.dependents] } : {}),
  }
}

function mergeIntoDependentNode (target: DependentNode, source: DependentNode): void {
  target.displayName ??= source.displayName
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
  return lexCompare(left.peersSuffixHash ?? '', right.peersSuffixHash ?? '')
}
