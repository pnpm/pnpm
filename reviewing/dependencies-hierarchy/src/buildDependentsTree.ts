import path from 'path'
import {
  getLockfileImporterId,
  type LockfileObject,
  type ProjectSnapshot,
  type PackageSnapshots,
} from '@pnpm/lockfile.fs'
import { nameVerFromPkgSnapshot } from '@pnpm/lockfile.utils'
import { readModulesManifest } from '@pnpm/modules-yaml'
import { normalizeRegistries } from '@pnpm/normalize-registries'
import { type DependenciesField, type DependencyManifest, type Finder, type Registries } from '@pnpm/types'
import { lexCompare } from '@pnpm/util.lex-comparator'
import semver from 'semver'
import realpathMissing from 'realpath-missing'
import { buildDependencyGraph, type DependencyGraph } from './buildDependencyGraph.js'
import { createPackagesSearcher } from './createPackagesSearcher.js'
import { peersSuffixHashFromDepPath } from './peersSuffixHash.js'
import { type TreeNodeId } from './TreeNodeId.js'
import { getPkgInfo } from './getPkgInfo.js'

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
  nameFormatter?: (info: { name: string, version: string, manifest: DependencyManifest }) => string | undefined
  /** Tracks nodes on the current path for cycle detection. Mutated during walk. */
  visited: Set<string>
  /** Tracks nodes already fully expanded, for deduplication across branches. */
  expanded: Set<string>
}

export async function buildDependentsTree (
  packages: string[],
  projectPaths: string[],
  opts: {
    lockfileDir: string
    include?: { [field in DependenciesField]?: boolean }
    modulesDir?: string
    registries?: Registries
    finders?: Finder[]
    importerInfoMap: Map<string, ImporterInfo>
    lockfile: LockfileObject
    nameFormatter?: (info: { name: string, version: string, manifest: DependencyManifest }) => string | undefined
  }
): Promise<DependentsTree[]> {
  const modulesDir = await realpathMissing(path.join(opts.lockfileDir, opts.modulesDir ?? 'node_modules'))
  const modules = await readModulesManifest(modulesDir)
  const registries = normalizeRegistries({
    ...opts.registries,
    ...modules?.registries,
  })
  const storeDir = modules?.storeDir
  const virtualStoreDir = modules?.virtualStoreDir ?? path.join(modulesDir, '.pnpm')
  const virtualStoreDirMaxLength = modules?.virtualStoreDirMaxLength ?? 120

  const include = opts.include ?? {
    dependencies: true,
    devDependencies: true,
    optionalDependencies: true,
  }

  // Build root IDs from the selected project paths (respects --filter / --recursive)
  const allRootIds: TreeNodeId[] = []
  for (const projectPath of projectPaths) {
    const importerId = getLockfileImporterId(opts.lockfileDir, projectPath)
    if (opts.lockfile.importers[importerId]) {
      allRootIds.push({ type: 'importer', importerId })
    }
  }

  const graph = buildDependencyGraph(allRootIds, {
    currentPackages: opts.lockfile.packages ?? {},
    importers: opts.lockfile.importers,
    include,
    lockfileDir: opts.lockfileDir,
  })

  const reverseMap = invertGraph(graph)
  const search = createPackagesSearcher(packages, opts.finders)
  const currentPackages = opts.lockfile.packages ?? {}

  // Pre-compute resolved filesystem paths for all package nodes by walking the
  // graph top-down from importers.  This is needed for global virtual store
  // where symlinks must be resolved through each parent's node_modules.
  const resolvedPackageNodes = resolvePackageNodes(graph, currentPackages, {
    virtualStoreDir,
    virtualStoreDirMaxLength,
    modulesDir,
    registries,
    wantedPackages: currentPackages,
    storeDir,
  })

  // Scan all package nodes for matches.
  // A package matches if any of the aliases used to refer to it (from incoming
  // edges in the graph) or its canonical name match the search query.
  // Each distinct depPath (i.e. different peer dep resolutions) is kept as a
  // separate result so that peer variants are visible in the output.
  const trees: DependentsTree[] = []
  const ctx: WalkContext = {
    reverseMap,
    graph,
    importers: opts.lockfile.importers,
    currentPackages,
    importerInfoMap: opts.importerInfoMap,
    resolvedPackageNodes,
    nameFormatter: opts.nameFormatter,
    visited: new Set(),
    expanded: new Set(),
  }

  for (const [serialized, node] of graph.nodes) {
    if (node.nodeId.type !== 'package') continue
    const depPath = node.nodeId.depPath
    const snapshot = currentPackages[depPath]
    if (snapshot == null) continue

    const { name, version } = nameVerFromPkgSnapshot(depPath, snapshot)
    const pkgNode = resolvedPackageNodes.get(serialized)
    if (!pkgNode) continue
    const readManifest = pkgNode.readManifest

    // Check canonical name first
    let matched = search({ alias: name, name, version, readManifest })

    // Also check aliases from incoming edges (handles npm: protocol aliases)
    if (!matched) {
      const incomingEdges = reverseMap.get(serialized)
      if (incomingEdges) {
        for (const edge of incomingEdges) {
          if (edge.alias !== name) {
            matched = search({ alias: edge.alias, name, version, readManifest })
            if (matched) break
          }
        }
      }
    }
    if (!matched) continue

    ctx.visited = new Set([serialized])
    ctx.expanded = new Set()
    const dependents = walkReverse(serialized, ctx)
    const peersSuffixHash = peersSuffixHashFromDepPath(depPath)

    const displayName = opts.nameFormatter
      ? opts.nameFormatter({ name, version, manifest: readManifest() })
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
    trees.push(tree)
  }

  trees.sort((a, b) => {
    const nameCmp = lexCompare(a.name, b.name)
    if (nameCmp !== 0) return nameCmp
    const versionCmp = semver.valid(a.version) && semver.valid(b.version)
      ? semver.compare(a.version, b.version)
      : lexCompare(a.version, b.version)
    if (versionCmp !== 0) return versionCmp
    return lexCompare(a.peersSuffixHash ?? '', b.peersSuffixHash ?? '')
  })
  return trees
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
    registries: Registries
    wantedPackages: PackageSnapshots
    storeDir?: string
  }
): Map<string, { path: string, readManifest: () => DependencyManifest }> {
  const resolved = new Map<string, { path: string, readManifest: () => DependencyManifest }>()

  function walk (serialized: string, parentDir: string | undefined): void {
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
        linkedPathBaseDir: opts.modulesDir, // This might need adjustment for linked deps?
        parentDir,
        ref: edge.target.nodeId.depPath,
        skipped: new Set(),
      })

      resolved.set(childSerialized, { path: pkgInfo.path, readManifest })
      walk(childSerialized, pkgInfo.path)
    }
  }

  for (const [serialized, node] of graph.nodes) {
    if (node.nodeId.type === 'importer') {
      walk(serialized, undefined)
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
  const sortedEdges = [...reverseEdges].sort((a, b) => {
    const cmp = lexCompare(resolveParentName(a, ctx), resolveParentName(b, ctx))
    if (cmp !== 0) return cmp
    return lexCompare(a.parentSerialized, b.parentSerialized)
  })

  const dependents: DependentNode[] = []

  for (const edge of sortedEdges) {
    // Cycle detection: this node is already on our current path
    if (ctx.visited.has(edge.parentSerialized)) {
      const parentNode = ctx.graph.nodes.get(edge.parentSerialized)
      if (parentNode?.nodeId.type === 'importer') {
        const info = ctx.importerInfoMap.get(parentNode.nodeId.importerId)
        if (info) {
          dependents.push({
            name: info.name,
            version: info.version,
            circular: true,
          })
        }
      } else if (parentNode?.nodeId.type === 'package') {
        const snapshot = ctx.currentPackages[parentNode.nodeId.depPath]
        if (snapshot) {
          const { name, version } = nameVerFromPkgSnapshot(parentNode.nodeId.depPath, snapshot)
          const displayName = resolveDisplayName(edge.parentSerialized, name, version, ctx)
          dependents.push({ name, displayName, version, circular: true })
        }
      }
      continue
    }

    const parentGraphNode = ctx.graph.nodes.get(edge.parentSerialized)
    if (parentGraphNode == null) continue

    if (parentGraphNode.nodeId.type === 'importer') {
      const importerId = parentGraphNode.nodeId.importerId
      const info = ctx.importerInfoMap.get(importerId) ?? { name: importerId, version: '' }
      const depField = getDepFieldForAlias(edge.alias, ctx.importers[importerId])
      dependents.push({
        name: info.name,
        version: info.version,
        depField,
      })
    } else if (parentGraphNode.nodeId.type === 'package') {
      const snapshot = ctx.currentPackages[parentGraphNode.nodeId.depPath]
      if (snapshot == null) continue
      const { name, version } = nameVerFromPkgSnapshot(parentGraphNode.nodeId.depPath, snapshot)
      const peersSuffixHash = peersSuffixHashFromDepPath(parentGraphNode.nodeId.depPath)

      // Deduplication: if this package was already expanded elsewhere in the
      // tree, show it as a leaf to keep the output bounded.
      const displayName = resolveDisplayName(edge.parentSerialized, name, version, ctx)

      if (ctx.expanded.has(edge.parentSerialized)) {
        dependents.push({ name, displayName, version, peersSuffixHash, deduped: true })
        continue
      }

      ctx.visited.add(edge.parentSerialized)
      ctx.expanded.add(edge.parentSerialized)
      const childDependents = walkReverse(edge.parentSerialized, ctx)
      ctx.visited.delete(edge.parentSerialized)

      dependents.push({
        name,
        displayName,
        version,
        peersSuffixHash,
        dependents: childDependents.length > 0 ? childDependents : undefined,
      })
    }
  }

  return dependents
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
  if (importerSnapshot.devDependencies?.[alias] != null) return 'devDependencies'
  if (importerSnapshot.optionalDependencies?.[alias] != null) return 'optionalDependencies'
  if (importerSnapshot.dependencies?.[alias] != null) return 'dependencies'
  return undefined
}
