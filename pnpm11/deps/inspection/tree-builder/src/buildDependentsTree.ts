import path from 'node:path'

import { normalizeRegistriesByScope } from '@pnpm/config.normalize-registries'
import { withCollapsedVariants } from '@pnpm/deps.path'
import { readModulesManifest } from '@pnpm/installing.modules-yaml'
import {
  getLockfileImporterId,
  type LockfileObject,
  type PackageSnapshots,
  type ProjectSnapshot,
  type ResolvedDependencies,
} from '@pnpm/lockfile.fs'
import { getPeerSatisfactionEdgesToSkip } from '@pnpm/lockfile.peer-edges'
import { nameVerFromPkgSnapshot } from '@pnpm/lockfile.utils'
import { StoreIndex } from '@pnpm/store.index'
import { lexCompare } from '@pnpm/text.ordinal-comparator'
import type { DependenciesField, DependencyManifest, DepPath, Finder, RegistriesByScope } from '@pnpm/types'
import { realpathMissing } from 'realpath-missing'
import semver from 'semver'

import { buildDependencyGraph, type DependencyGraph } from './buildDependencyGraph.js'
import { createPackagesSearcher } from './createPackagesSearcher.js'
import { getPkgInfo } from './getPkgInfo.js'
import { peersSuffixHashFromDepPath } from './peersSuffixHash.js'
import type { TreeNodeId } from './TreeNodeId.js'

interface ReverseEdge {
  parentSerialized: string
  parentNodeId: TreeNodeId
  alias: string
}

export interface DependentNode {
  name: string
  displayName?: string
  version: string
  dependents?: DependentNode[]
  circular?: true
  deduped?: true
  /** Short hash distinguishing peer-dep variants of the same name@version */
  peersSuffixHash?: string
  /** For importer leaf nodes: which dep field */
  depField?: DependenciesField
}

export interface DependentsTree {
  name: string
  displayName?: string
  version: string
  /** Resolved filesystem path to this package */
  path?: string
  /** Short hash distinguishing peer-dep variants of the same name@version */
  peersSuffixHash?: string
  /** Message returned by the finder function, if any */
  searchMessage?: string
  dependents: DependentNode[]
}

export interface ImporterInfo {
  name: string
  version: string
}

interface WalkContext {
  reverseMap: Map<string, ReverseEdge[]>
  graph: DependencyGraph
  importers: Record<string, ProjectSnapshot>
  currentPackages: PackageSnapshots
  importerInfoMap: Map<string, ImporterInfo>
  resolvedPackageNodes: Map<string, { path: string, readManifest: () => DependencyManifest }>
  nameFormatter?: NameFormatter
  /** Tracks nodes on the current path for cycle detection. Mutated during walk. */
  visited: Set<string>
  /** Tracks nodes already fully expanded, for deduplication across branches. */
  expanded: Set<string>
}

type NameFormatter = (info: { name: string, version: string, manifest: DependencyManifest }) => string | undefined

interface BuildDependentsTreeOptions {
  lockfileDir: string
  include?: { [field in DependenciesField]?: boolean }
  modulesDir?: string
  registriesByScope?: RegistriesByScope
  registriesByPrefix?: Record<string, string>
  finders?: Finder[]
  importerInfoMap: Map<string, ImporterInfo>
  lockfile: LockfileObject
  nameFormatter?: NameFormatter
  resolvePeersFromWorkspaceRoot?: boolean
  nodeLinker?: 'hoisted' | 'isolated' | 'pnp'
}

export async function buildDependentsTree (
  packages: string[],
  projectPaths: string[],
  opts: BuildDependentsTreeOptions
): Promise<DependentsTree[]> {
  const modulesDir = await realpathMissing(path.join(opts.lockfileDir, opts.modulesDir ?? 'node_modules'))
  const modules = await readModulesManifest(modulesDir)
  const registriesByScope = normalizeRegistriesByScope(opts.registriesByScope)
  const storeDir = modules?.storeDir
  const storeIndex = storeDir ? new StoreIndex(storeDir) : undefined

  const graph = buildSelectedProjectsGraph(projectPaths, opts)
  const reverseMap = invertGraph(graph)
  const search = createPackagesSearcher(packages, opts.finders)
  const currentPackages = opts.lockfile.packages ?? {}

  // Pre-compute resolved filesystem paths for all package nodes by walking the
  // graph top-down from importers.  This is needed for global virtual store
  // where symlinks must be resolved through each parent's node_modules.
  const resolvedPackageNodes = resolvePackageNodes(graph, currentPackages, {
    virtualStoreDir: modules?.virtualStoreDir ?? path.join(modulesDir, '.pnpm'),
    virtualStoreDirMaxLength: modules?.virtualStoreDirMaxLength ?? 120,
    modulesDir,
    registriesByScope,
    registriesByPrefix: opts.registriesByPrefix,
    wantedPackages: currentPackages,
    storeDir,
    storeIndex,
    nodeLinker: modules?.nodeLinker ?? opts.nodeLinker,
    hoistedLocations: modules?.hoistedLocations && withCollapsedVariants(modules.hoistedLocations),
    lockfileDir: opts.lockfileDir,
  })

  const trees = collectMatchedTrees(search, {
    reverseMap,
    graph,
    importers: opts.lockfile.importers,
    currentPackages,
    importerInfoMap: opts.importerInfoMap,
    resolvedPackageNodes,
    nameFormatter: opts.nameFormatter,
    visited: new Set(),
    expanded: new Set(),
  })
  trees.sort(compareDependentsTrees)
  storeIndex?.close()
  return trees
}

function buildSelectedProjectsGraph (projectPaths: string[], opts: BuildDependentsTreeOptions): DependencyGraph {
  const include = opts.include ?? {
    dependencies: true,
    devDependencies: true,
    optionalDependencies: true,
  }

  const allRootIds: TreeNodeId[] = []
  for (const projectPath of projectPaths) {
    const importerId = getLockfileImporterId(opts.lockfileDir, projectPath)
    if (opts.lockfile.importers[importerId]) {
      allRootIds.push({ type: 'importer', importerId })
    }
  }

  return buildDependencyGraph(allRootIds, {
    currentPackages: opts.lockfile.packages ?? {},
    importers: opts.lockfile.importers,
    include,
    lockfileDir: opts.lockfileDir,
    peerSatisfactionEdges: getPeerSatisfactionEdgesToSkip(opts.lockfile, {
      include,
      resolvePeersFromWorkspaceRoot: opts.resolvePeersFromWorkspaceRoot,
    }),
  })
}

// Scan all package nodes for matches.
// A package matches if any of the aliases used to refer to it (from incoming
// edges in the graph) or its canonical name match the search query.
// Each distinct depPath (i.e. different peer dep resolutions) is kept as a
// separate result so that peer variants are visible in the output.
function collectMatchedTrees (search: Finder, ctx: WalkContext): DependentsTree[] {
  const trees: DependentsTree[] = []
  for (const [serialized, node] of ctx.graph.nodes) {
    if (node.nodeId.type !== 'package') continue
    const tree = buildTreeIfMatched(search, { serialized, depPath: node.nodeId.depPath }, ctx)
    if (tree != null) trees.push(tree)
  }
  return trees
}

function buildTreeIfMatched (
  search: Finder,
  { serialized, depPath }: { serialized: string, depPath: DepPath },
  ctx: WalkContext
): DependentsTree | undefined {
  const snapshot = ctx.currentPackages[depPath]
  if (snapshot == null) return undefined

  const { name, version } = nameVerFromPkgSnapshot(depPath, snapshot)
  const pkgNode = ctx.resolvedPackageNodes.get(serialized)
  if (!pkgNode) return undefined
  const readManifest = pkgNode.readManifest

  const matched = searchPackage(search, { serialized, name, version, readManifest }, ctx)
  if (!matched) return undefined

  ctx.visited = new Set([serialized])
  ctx.expanded = new Set()
  const dependents = walkReverse(serialized, ctx)
  const peersSuffixHash = peersSuffixHashFromDepPath(depPath)

  const displayName = ctx.nameFormatter
    ? ctx.nameFormatter({ name, version, manifest: readManifest() })
    : undefined
  const tree: DependentsTree = {
    name,
    displayName,
    version,
    path: pkgNode.path,
    peersSuffixHash,
    dependents,
  }
  if (typeof matched === 'string') {
    tree.searchMessage = matched
  }
  return tree
}

function searchPackage (
  search: Finder,
  pkg: { serialized: string, name: string, version: string, readManifest: () => DependencyManifest },
  ctx: WalkContext
): ReturnType<Finder> {
  const { name, version, readManifest } = pkg
  const matched = search({ alias: name, name, version, readManifest })
  if (matched) return matched

  // Also check aliases from incoming edges (handles npm: protocol aliases)
  for (const edge of ctx.reverseMap.get(pkg.serialized) ?? []) {
    if (edge.alias === name) continue
    const aliasMatched = search({ alias: edge.alias, name, version, readManifest })
    if (aliasMatched) return aliasMatched
  }
  return false
}

function compareDependentsTrees (left: DependentsTree, right: DependentsTree): number {
  const nameCmp = lexCompare(left.name, right.name)
  if (nameCmp !== 0) return nameCmp
  const versionCmp = semver.valid(left.version) && semver.valid(right.version)
    ? semver.compare(left.version, right.version)
    : lexCompare(left.version, right.version)
  if (versionCmp !== 0) return versionCmp
  return lexCompare(left.peersSuffixHash ?? '', right.peersSuffixHash ?? '')
}

function invertGraph (graph: DependencyGraph): Map<string, ReverseEdge[]> {
  const reverse = new Map<string, ReverseEdge[]>()
  for (const [parentSerialized, node] of graph.nodes) {
    for (const edge of node.edges) {
      if (edge.target == null) continue
      const childSerialized = edge.target.id
      let entries = reverse.get(childSerialized)
      if (entries == null) {
        entries = []
        reverse.set(childSerialized, entries)
      }
      entries.push({
        parentSerialized,
        parentNodeId: node.nodeId,
        alias: edge.alias,
      })
    }
  }
  return reverse
}

/**
 * Walks the dependency graph top-down from importer nodes and resolves the
 * filesystem path for every package node.  This is necessary for global virtual
 * store where the correct path can only be obtained by following symlinks
 * through each parent's node_modules directory.
 */
function resolvePackageNodes (
  graph: DependencyGraph,
  currentPackages: PackageSnapshots,
  opts: {
    virtualStoreDir: string
    virtualStoreDirMaxLength: number
    modulesDir: string
    registriesByScope: RegistriesByScope
    registriesByPrefix?: Record<string, string>
    wantedPackages: PackageSnapshots
    storeDir?: string
    storeIndex?: StoreIndex
    nodeLinker?: 'hoisted' | 'isolated' | 'pnp'
    hoistedLocations?: Record<string, string[]>
    lockfileDir: string
  }
): Map<string, { path: string, readManifest: () => DependencyManifest }> {
  const resolved = new Map<string, { path: string, readManifest: () => DependencyManifest }>()

  function walk (serialized: string, parentDir: string | undefined, importerDir: string): void {
    const node = graph.nodes.get(serialized)
    if (!node) return
    for (const edge of node.edges) {
      if (edge.target == null) continue
      const childSerialized = edge.target.id
      if (resolved.has(childSerialized)) continue
      if (edge.target.nodeId.type !== 'package') continue

      const { pkgInfo, readManifest } = getPkgInfo({
        ...opts,
        alias: edge.alias,
        currentPackages,
        depTypes: {},
        linkedPathBaseDir: importerDir,
        parentDir,
        ref: edge.target.nodeId.depPath,
        skipped: new Set(),
      })

      resolved.set(childSerialized, { path: pkgInfo.path, readManifest })
      walk(childSerialized, pkgInfo.path, importerDir)
    }
  }

  for (const [serialized, node] of graph.nodes) {
    if (node.nodeId.type === 'importer') {
      walk(serialized, undefined, path.join(opts.lockfileDir, node.nodeId.importerId))
    }
  }

  return resolved
}

function walkReverse (
  nodeId: string,
  ctx: WalkContext
): DependentNode[] {
  const reverseEdges = ctx.reverseMap.get(nodeId)
  if (reverseEdges == null || reverseEdges.length === 0) return []

  // Sort edges by parent name (with serialized ID as tiebreaker) so that
  // deduplication is deterministic: the first parent always gets fully expanded.
  const sortedEdges = [...reverseEdges].sort((left, right) => {
    const cmp = lexCompare(resolveParentName(left, ctx), resolveParentName(right, ctx))
    if (cmp !== 0) return cmp
    return lexCompare(left.parentSerialized, right.parentSerialized)
  })

  const dependents: DependentNode[] = []

  for (const edge of sortedEdges) {
    // Cycle detection: this node is already on our current path
    const dependent = ctx.visited.has(edge.parentSerialized)
      ? createCircularDependent(edge, ctx)
      : createDependent(edge, ctx)
    if (dependent != null) dependents.push(dependent)
  }

  return dependents
}

function createCircularDependent (edge: ReverseEdge, ctx: WalkContext): DependentNode | undefined {
  const parentNode = ctx.graph.nodes.get(edge.parentSerialized)
  if (parentNode?.nodeId.type === 'importer') {
    const info = ctx.importerInfoMap.get(parentNode.nodeId.importerId)
    if (!info) return undefined
    return {
      name: info.name,
      version: info.version,
      circular: true,
    }
  }
  if (parentNode?.nodeId.type !== 'package') return undefined
  const snapshot = ctx.currentPackages[parentNode.nodeId.depPath]
  if (!snapshot) return undefined
  const { name, version } = nameVerFromPkgSnapshot(parentNode.nodeId.depPath, snapshot)
  const displayName = resolveDisplayName(edge.parentSerialized, name, version, ctx)
  return { name, displayName, version, circular: true }
}

function createDependent (edge: ReverseEdge, ctx: WalkContext): DependentNode | undefined {
  const parentGraphNode = ctx.graph.nodes.get(edge.parentSerialized)
  if (parentGraphNode == null) return undefined

  const parentNodeId = parentGraphNode.nodeId
  if (parentNodeId.type === 'importer') {
    const importerId = parentNodeId.importerId
    const info = ctx.importerInfoMap.get(importerId) ?? { name: importerId, version: '' }
    const depField = getDepFieldForAlias(edge.alias, ctx.importers[importerId])
    return {
      name: info.name,
      version: info.version,
      depField,
    }
  }
  if (parentNodeId.type === 'package') {
    return createPackageDependent(edge, parentNodeId.depPath, ctx)
  }
  return undefined
}

function createPackageDependent (edge: ReverseEdge, depPath: DepPath, ctx: WalkContext): DependentNode | undefined {
  const snapshot = ctx.currentPackages[depPath]
  if (snapshot == null) return undefined
  const { name, version } = nameVerFromPkgSnapshot(depPath, snapshot)
  const peersSuffixHash = peersSuffixHashFromDepPath(depPath)

  // Deduplication: if this package was already expanded elsewhere in the
  // tree, show it as a leaf to keep the output bounded.
  const displayName = resolveDisplayName(edge.parentSerialized, name, version, ctx)

  if (ctx.expanded.has(edge.parentSerialized)) {
    return { name, displayName, version, peersSuffixHash, deduped: true }
  }

  ctx.visited.add(edge.parentSerialized)
  ctx.expanded.add(edge.parentSerialized)
  const childDependents = walkReverse(edge.parentSerialized, ctx)
  ctx.visited.delete(edge.parentSerialized)

  return {
    name,
    displayName,
    version,
    peersSuffixHash,
    dependents: childDependents.length > 0 ? childDependents : undefined,
  }
}

function resolveParentName (edge: ReverseEdge, ctx: WalkContext): string {
  const graphNode = ctx.graph.nodes.get(edge.parentSerialized)
  if (graphNode == null) return ''
  if (graphNode.nodeId.type === 'importer') {
    const info = ctx.importerInfoMap.get(graphNode.nodeId.importerId)
    return info?.name ?? graphNode.nodeId.importerId
  }
  const snapshot = ctx.currentPackages[graphNode.nodeId.depPath]
  if (snapshot == null) return ''
  return nameVerFromPkgSnapshot(graphNode.nodeId.depPath, snapshot).name
}

function resolveDisplayName (serialized: string, name: string, version: string, ctx: WalkContext): string | undefined {
  if (!ctx.nameFormatter) return undefined
  const pkgNode = ctx.resolvedPackageNodes.get(serialized)
  if (!pkgNode) return undefined
  return ctx.nameFormatter({ name, version, manifest: pkgNode.readManifest() })
}

function getDepFieldForAlias (
  alias: string,
  importerSnapshot: ProjectSnapshot
): DependenciesField | undefined {
  if (hasDependency(importerSnapshot.devDependencies, alias)) return 'devDependencies'
  if (hasDependency(importerSnapshot.optionalDependencies, alias)) return 'optionalDependencies'
  if (hasDependency(importerSnapshot.dependencies, alias)) return 'dependencies'
  return undefined
}

function hasDependency (dependencies: ResolvedDependencies | undefined, alias: string): boolean {
  return dependencies != null && Object.hasOwn(dependencies, alias) && dependencies[alias] != null
}
