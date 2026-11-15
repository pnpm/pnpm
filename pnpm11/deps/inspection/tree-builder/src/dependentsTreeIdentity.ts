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

export interface GetPackageMergeIdOpts {
  lockfileDir: string
  depPath: DepPath
  snapshot: PackageSnapshot
  version: string | undefined
}

export function getPackageMergeId (opts: GetPackageMergeIdOpts): string | undefined {
  if (opts.version) return undefined
  if ('directory' in opts.snapshot.resolution && opts.snapshot.resolution.type === 'directory') {
    const { directory } = opts.snapshot.resolution
    if (directory.startsWith('\\\\') || directory.startsWith('//')) {
      return `dir:${normalizeUncDirectory(directory)}`
    }
    const normalized = path.isAbsolute(directory)
      ? path.normalize(directory)
      : path.join(opts.lockfileDir, directory)
    return `dir:${trimTrailingSeparators(normalized)}`
  }
  return `dep:${removePeersSuffix(opts.depPath)}`
}

function normalizeUncDirectory (directory: string): string {
  const parts = directory.replace(/^[/\\]+/, '').split(/[/\\]+/).filter(Boolean)
  const head = parts.slice(0, 2)
  const tail: string[] = []
  for (const segment of parts.slice(2)) {
    if (segment === '.') continue
    if (segment === '..') {
      tail.pop()
    } else {
      tail.push(segment)
    }
  }
  return `//${[...head, ...tail].join('/')}`
}

function trimTrailingSeparators (normalizedPath: string): string {
  const { root } = path.parse(normalizedPath)
  let end = normalizedPath.length
  while (end > root.length && normalizedPath[end - 1] === path.sep) {
    end--
  }
  return normalizedPath.slice(0, end)
}

export interface RecordPackageNodeMergeIdOpts {
  node: DependentNode
  lockfileDir: string
  depPath: DepPath
  snapshot: PackageSnapshot
}

export function recordPackageNodeMergeId (opts: RecordPackageNodeMergeIdOpts): void {
  const mergeId = getPackageMergeId({
    lockfileDir: opts.lockfileDir,
    depPath: opts.depPath,
    snapshot: opts.snapshot,
    version: opts.node.version,
  })
  if (mergeId != null) {
    setDependentNodeMergeId(opts.node, mergeId)
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
