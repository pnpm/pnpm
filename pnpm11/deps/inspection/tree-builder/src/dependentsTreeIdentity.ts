import fs from 'node:fs'
import path from 'node:path'

import { removePeersSuffix } from '@pnpm/deps.path'
import type { PackageSnapshot } from '@pnpm/lockfile.fs'
import { lexCompare } from '@pnpm/text.ordinal-comparator'
import type { DepPath } from '@pnpm/types'
import semver from 'semver'

import type { DependentNode, DependentsTree } from './buildDependentsTree.js'

const dependentNodeImporterIds = new WeakMap<DependentNode, string>()
const dependentsTreeMergeIds = new WeakMap<DependentsTree, string>()
const dependentNodeMergeIds = new WeakMap<DependentNode, string>()

export function setDependentNodeImporterId (node: DependentNode, importerId: string): void {
  dependentNodeImporterIds.set(node, importerId)
}

export function getDependentNodeImporterId (node: DependentNode): string | undefined {
  return dependentNodeImporterIds.get(node)
}

export function setDependentsTreeMergeId (tree: DependentsTree, mergeId: string): void {
  dependentsTreeMergeIds.set(tree, mergeId)
}

export function getDependentsTreeMergeId (tree: DependentsTree): string | undefined {
  return dependentsTreeMergeIds.get(tree)
}

export function setDependentNodeMergeId (node: DependentNode, mergeId: string): void {
  dependentNodeMergeIds.set(node, mergeId)
}

export function getDependentNodeMergeId (node: DependentNode): string | undefined {
  return dependentNodeMergeIds.get(node)
}

export function getPackageMergeId (
  lockfileDir: string,
  depPath: DepPath,
  snapshot: PackageSnapshot,
  version: string | undefined
): string | undefined {
  if (version) return undefined
  if ('directory' in snapshot.resolution && snapshot.resolution.type === 'directory') {
    const joined = path.resolve(lockfileDir, snapshot.resolution.directory)
    try {
      return `dir:${fs.realpathSync.native(joined)}`
    } catch {
      return `dir:${joined}`
    }
  }
  return `dep:${removePeersSuffix(depPath)}`
}

export function recordPackageNodeMergeId (
  node: DependentNode,
  lockfileDir: string,
  depPath: DepPath,
  snapshot: PackageSnapshot
): void {
  const mergeId = getPackageMergeId(lockfileDir, depPath, snapshot, node.version)
  if (mergeId != null) {
    setDependentNodeMergeId(node, mergeId)
  }
}

export function compareDependentsTrees (left: DependentsTree, right: DependentsTree): number {
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
  return lexCompare(getDependentsTreeMergeId(left) ?? '', getDependentsTreeMergeId(right) ?? '')
}
