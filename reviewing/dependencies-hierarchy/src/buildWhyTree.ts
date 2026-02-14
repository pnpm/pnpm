import path from 'path'
import {
  getLockfileImporterId,
  readCurrentLockfile,
  readWantedLockfile,
  type ProjectSnapshot,
  type PackageSnapshots,
} from '@pnpm/lockfile.fs'
import { nameVerFromPkgSnapshot } from '@pnpm/lockfile.utils'
import { readModulesManifest } from '@pnpm/modules-yaml'
import { type DependenciesField, type DependencyManifest, type Finder } from '@pnpm/types'
import { lexCompare } from '@pnpm/util.lex-comparator'
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

export interface WhyDependant {
  name: string
  version: string
  dependants?: WhyDependant[]
  circular?: true
  deduped?: true
  /** Short hash distinguishing peer-dep variants of the same name@version */
  peersSuffixHash?: string
  /** For importer leaf nodes: which dep field */
  depField?: DependenciesField
}

export interface WhyPackageResult {
  name: string
  version: string
  /** Resolved filesystem path to this package */
  path?: string
  /** Short hash distinguishing peer-dep variants of the same name@version */
  peersSuffixHash?: string
  /** Message returned by the finder function, if any */
  searchMessage?: string
  dependants: WhyDependant[]
}

export interface ImporterInfo {
  name: string
  version: string
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

function getDepFieldForAlias (
  alias: string,
  importerSnapshot: ProjectSnapshot
): DependenciesField | undefined {
  if (importerSnapshot.devDependencies?.[alias] != null) return 'devDependencies'
  if (importerSnapshot.optionalDependencies?.[alias] != null) return 'optionalDependencies'
  if (importerSnapshot.dependencies?.[alias] != null) return 'dependencies'
  return undefined
}

interface WalkContext {
  reverseMap: Map<string, ReverseEdge[]>
  graph: DependencyGraph
  importers: Record<string, ProjectSnapshot>
  currentPackages: PackageSnapshots
  importerInfoMap: Map<string, ImporterInfo>
  /** Tracks nodes on the current path for cycle detection. Mutated during walk. */
  visited: Set<string>
  /** Tracks nodes already fully expanded, for deduplication across branches. */
  expanded: Set<string>
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

function walkReverse (
  nodeId: string,
  ctx: WalkContext
): WhyDependant[] {
  const reverseEdges = ctx.reverseMap.get(nodeId)
  if (reverseEdges == null || reverseEdges.length === 0) return []

  // Sort edges by parent name so that deduplication is deterministic:
  // the alphabetically-first parent always gets fully expanded.
  const sortedEdges = [...reverseEdges].sort((a, b) =>
    lexCompare(resolveParentName(a, ctx), resolveParentName(b, ctx))
  )

  const dependants: WhyDependant[] = []

  for (const edge of sortedEdges) {
    // Cycle detection: this node is already on our current path
    if (ctx.visited.has(edge.parentSerialized)) {
      const parentNode = ctx.graph.nodes.get(edge.parentSerialized)
      if (parentNode?.nodeId.type === 'importer') {
        const info = ctx.importerInfoMap.get(parentNode.nodeId.importerId)
        if (info) {
          dependants.push({
            name: info.name,
            version: info.version,
            circular: true,
          })
        }
      } else if (parentNode?.nodeId.type === 'package') {
        const snapshot = ctx.currentPackages[parentNode.nodeId.depPath]
        if (snapshot) {
          const { name, version } = nameVerFromPkgSnapshot(parentNode.nodeId.depPath, snapshot)
          dependants.push({ name, version, circular: true })
        }
      }
      continue
    }

    const parentGraphNode = ctx.graph.nodes.get(edge.parentSerialized)
    if (parentGraphNode == null) continue

    if (parentGraphNode.nodeId.type === 'importer') {
      const importerId = parentGraphNode.nodeId.importerId
      const info = ctx.importerInfoMap.get(importerId)!
      const depField = getDepFieldForAlias(edge.alias, ctx.importers[importerId])
      dependants.push({
        name: info.name,
        version: info.version,
        depField,
      })
    } else if (parentGraphNode.nodeId.type === 'package') {
      const snapshot = ctx.currentPackages[parentGraphNode.nodeId.depPath]
      if (snapshot == null) continue
      const { name, version } = nameVerFromPkgSnapshot(parentGraphNode.nodeId.depPath, snapshot)
      const peerHash = peersSuffixHashFromDepPath(parentGraphNode.nodeId.depPath)

      // Deduplication: if this package was already expanded elsewhere in the
      // tree, show it as a leaf to keep the output bounded.
      if (ctx.expanded.has(edge.parentSerialized)) {
        dependants.push({ name, version, peersSuffixHash: peerHash, deduped: true })
        continue
      }

      ctx.visited.add(edge.parentSerialized)
      ctx.expanded.add(edge.parentSerialized)
      const childDependants = walkReverse(edge.parentSerialized, ctx)
      ctx.visited.delete(edge.parentSerialized)

      dependants.push({
        name,
        version,
        peersSuffixHash: peerHash,
        dependants: childDependants.length > 0 ? childDependants : undefined,
      })
    }
  }

  return dependants
}

export async function buildWhyTrees (
  packages: string[],
  projectPaths: string[],
  opts: {
    lockfileDir: string
    include?: { [field in DependenciesField]?: boolean }
    modulesDir?: string
    checkWantedLockfileOnly?: boolean
    finders?: Finder[]
    importerInfoMap: Map<string, ImporterInfo>
  }
): Promise<WhyPackageResult[]> {
  const modulesDir = await realpathMissing(path.join(opts.lockfileDir, opts.modulesDir ?? 'node_modules'))
  const modules = await readModulesManifest(modulesDir)
  const internalPnpmDir = path.join(modulesDir, '.pnpm')
  const storeDir = modules?.storeDir
  const virtualStoreDir = modules?.virtualStoreDir ?? internalPnpmDir
  const virtualStoreDirMaxLength = modules?.virtualStoreDirMaxLength ?? 120
  const currentLockfile = await readCurrentLockfile(internalPnpmDir, { ignoreIncompatible: false })
  const wantedLockfile = await readWantedLockfile(opts.lockfileDir, { ignoreIncompatible: false })

  const lockfileToUse = opts.checkWantedLockfileOnly ? wantedLockfile : currentLockfile
  if (!lockfileToUse) return []

  const include = opts.include ?? {
    dependencies: true,
    devDependencies: true,
    optionalDependencies: true,
  }

  // Build root IDs from the selected project paths (respects --filter / --recursive)
  const allRootIds: TreeNodeId[] = []
  for (const projectPath of projectPaths) {
    const importerId = getLockfileImporterId(opts.lockfileDir, projectPath)
    if (lockfileToUse.importers[importerId]) {
      allRootIds.push({ type: 'importer', importerId })
    }
  }

  const graph = buildDependencyGraph(allRootIds, {
    currentPackages: lockfileToUse.packages ?? {},
    importers: lockfileToUse.importers,
    include,
    lockfileDir: opts.lockfileDir,
  })

  const reverseMap = invertGraph(graph)
  const search = createPackagesSearcher(packages, opts.finders)
  const currentPackages = lockfileToUse.packages ?? {}

  // Pre-compute resolved filesystem paths for all package nodes by walking the
  // graph top-down from importers.  This is needed for global virtual store
  // where symlinks must be resolved through each parent's node_modules.
  const resolvedPackageNodes = resolvePackageNodes(graph, currentPackages, {
    virtualStoreDir,
    virtualStoreDirMaxLength,
    modulesDir,
    wantedPackages: wantedLockfile?.packages ?? {},
    storeDir,
  })

  // Scan all package nodes for matches.
  // A package matches if any of the aliases used to refer to it (from incoming
  // edges in the graph) or its canonical name match the search query.
  // Each distinct depPath (i.e. different peer dep resolutions) is kept as a
  // separate result so that peer variants are visible in the output.
  const results: WhyPackageResult[] = []

  for (const [serialized, node] of graph.nodes) {
    if (node.nodeId.type !== 'package') continue
    const depPath = node.nodeId.depPath
    const snapshot = currentPackages[depPath]
    if (snapshot == null) continue

    const { name, version } = nameVerFromPkgSnapshot(depPath, snapshot)
    const readManifest = (): DependencyManifest => {
      const resolvedNode = resolvedPackageNodes.get(serialized)
      if (resolvedNode) {
        return resolvedNode.readManifest()
      }
      return { name, version } as DependencyManifest
    }

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

    const peerHash = peersSuffixHashFromDepPath(depPath)

    const ctx: WalkContext = {
      reverseMap,
      graph,
      importers: lockfileToUse.importers,
      currentPackages,
      importerInfoMap: opts.importerInfoMap,
      visited: new Set([serialized]),
      expanded: new Set(),
    }
    const dependants = walkReverse(serialized, ctx)

    const result: WhyPackageResult = { name, version, path: resolvedPackageNodes.get(serialized)?.path, peersSuffixHash: peerHash, dependants }
    if (typeof matched === 'string') {
      result.searchMessage = matched
    }
    results.push(result)
  }

  results.sort((a, b) => lexCompare(a.name, b.name))
  return results
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

      const pkgInfo = getPkgInfo({
        alias: edge.alias,
        ref: edge.target.nodeId.depPath,
        currentPackages,
        wantedPackages: opts.wantedPackages,
        storeDir: opts.storeDir,
        registries: {
          default: 'https://registry.npmjs.org/',
        },
        skipped: new Set(),
        depTypes: {},
        virtualStoreDir: opts.virtualStoreDir,
        virtualStoreDirMaxLength: opts.virtualStoreDirMaxLength,
        modulesDir: opts.modulesDir,
        parentDir,
        linkedPathBaseDir: opts.modulesDir, // This might need adjustment for linked deps?
      })

      resolved.set(childSerialized, { path: pkgInfo.pkgInfo.path, readManifest: pkgInfo.readManifest })
      walk(childSerialized, pkgInfo.pkgInfo.path)
    }
  }

  for (const [serialized, node] of graph.nodes) {
    if (node.nodeId.type === 'importer') {
      walk(serialized, undefined)
    }
  }

  return resolved
}
