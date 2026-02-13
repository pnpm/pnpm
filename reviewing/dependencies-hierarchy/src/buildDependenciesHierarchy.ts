import crypto from 'crypto'
import path from 'path'
import {
  getLockfileImporterId,
  type LockfileObject,
  type ProjectSnapshot,
  readCurrentLockfile,
  readWantedLockfile,
  type ResolvedDependencies,
} from '@pnpm/lockfile.fs'
import { detectDepTypes, type DepTypes } from '@pnpm/lockfile.detect-dep-types'
import { parseDepPath } from '@pnpm/dependency-path'
import { readModulesManifest } from '@pnpm/modules-yaml'
import { normalizeRegistries } from '@pnpm/normalize-registries'
import { readModulesDir } from '@pnpm/read-modules-dir'
import { safeReadPackageJsonFromDir, readPackageJsonFromDirSync } from '@pnpm/read-package-json'
import { type DependenciesField, type Finder, DEPENDENCIES_FIELDS, type Registries } from '@pnpm/types'
import normalizePath from 'normalize-path'
import realpathMissing from 'realpath-missing'
import resolveLinkTarget from 'resolve-link-target'
import { type PackageNode } from './PackageNode.js'
import { buildDependencyGraph, type DependencyGraph } from './buildDependencyGraph.js'
import { getTree, type GetTreeResult, type MaterializationCache } from './getTree.js'
import { getTreeNodeChildId } from './getTreeNodeChildId.js'
import { getPkgInfo } from './getPkgInfo.js'
import { type TreeNodeId } from './TreeNodeId.js'

export interface DependenciesHierarchy {
  dependencies?: PackageNode[]
  devDependencies?: PackageNode[]
  optionalDependencies?: PackageNode[]
  unsavedDependencies?: PackageNode[]
}

export async function buildDependenciesHierarchy (
  projectPaths: string[] | undefined,
  maybeOpts: {
    depth: number
    excludePeerDependencies?: boolean
    include?: { [dependenciesField in DependenciesField]: boolean }
    registries?: Registries
    onlyProjects?: boolean
    search?: Finder
    fastSearch?: (alias: string) => boolean
    showDedupedSearchMatches?: boolean
    lockfileDir: string
    checkWantedLockfileOnly?: boolean
    modulesDir?: string
    virtualStoreDirMaxLength: number
  }
): Promise<{ [projectDir: string]: DependenciesHierarchy }> {
  if (!maybeOpts?.lockfileDir) {
    throw new TypeError('opts.lockfileDir is required')
  }
  const modulesDir = await realpathMissing(path.join(maybeOpts.lockfileDir, maybeOpts.modulesDir ?? 'node_modules'))
  const modules = await readModulesManifest(modulesDir)
  const registries = normalizeRegistries({
    ...maybeOpts?.registries,
    ...modules?.registries,
  })
  const internalPnpmDir = path.join(modulesDir, '.pnpm')
  const currentLockfile = await readCurrentLockfile(internalPnpmDir, { ignoreIncompatible: false })
  const needsWantedLockfile = projectPaths == null || maybeOpts.checkWantedLockfileOnly
  const wantedLockfile = needsWantedLockfile
    ? await readWantedLockfile(maybeOpts.lockfileDir, { ignoreIncompatible: false })
    : null
  if (projectPaths == null) {
    projectPaths = Object.keys(wantedLockfile?.importers ?? {})
      .map((id) => path.join(maybeOpts.lockfileDir, id))
  }

  const result = {} as { [projectDir: string]: DependenciesHierarchy }

  const lockfileToUse = maybeOpts.checkWantedLockfileOnly ? wantedLockfile : currentLockfile

  if (!lockfileToUse) {
    for (const projectPath of projectPaths) {
      result[projectPath] = {}
    }
    return result
  }

  const opts = {
    depth: maybeOpts.depth || 0,
    excludePeerDependencies: maybeOpts.excludePeerDependencies,
    include: maybeOpts.include ?? {
      dependencies: true,
      devDependencies: true,
      optionalDependencies: true,
    },
    lockfileDir: maybeOpts.lockfileDir,
    checkWantedLockfileOnly: maybeOpts.checkWantedLockfileOnly,
    onlyProjects: maybeOpts.onlyProjects,
    registries,
    search: maybeOpts.search,
    showDedupedSearchMatches: maybeOpts.showDedupedSearchMatches ?? (maybeOpts.search != null),
    skipped: new Set(modules?.skipped ?? []),
    modulesDir,
    virtualStoreDir: modules?.virtualStoreDir,
    virtualStoreDirMaxLength: modules?.virtualStoreDirMaxLength ?? maybeOpts.virtualStoreDirMaxLength,
  }
  // Build the dependency graph ONCE for all importers and share a single
  // MaterializationCache so that identical subtrees are only materialized once.
  const allRootIds: TreeNodeId[] = projectPaths
    .map((projectPath) => {
      const importerId = getLockfileImporterId(opts.lockfileDir, projectPath)
      if (!lockfileToUse.importers[importerId]) return null
      return { type: 'importer' as const, importerId } as TreeNodeId
    })
    .filter((id): id is TreeNodeId => id != null)
  const sharedGraph = buildDependencyGraph(allRootIds, {
    currentPackages: lockfileToUse.packages ?? {},
    importers: lockfileToUse.importers,
    include: opts.include,
    lockfileDir: opts.lockfileDir,
  })
  const sharedMaterializationCache: MaterializationCache = new Map()
  const sharedDepTypes = detectDepTypes(lockfileToUse)

  // When searching, pre-filter importers to only those that can transitively
  // reach a package matching the search queries. This avoids per-importer work
  // (readModulesDir, getPkgInfo, getTree) for importers that can't match.
  const importerFilter = opts.search && maybeOpts.fastSearch
    ? findImportersReachingSearchTarget(sharedGraph, maybeOpts.fastSearch)
    : undefined

  const pairs = await Promise.all(projectPaths.map(async (projectPath) => {
    if (importerFilter) {
      const importerId = getLockfileImporterId(opts.lockfileDir, projectPath)
      if (!importerFilter.has(importerId)) {
        return [projectPath, {}] as [string, DependenciesHierarchy]
      }
    }
    return [
      projectPath,
      await dependenciesHierarchyForPackage(projectPath, lockfileToUse, wantedLockfile, opts, sharedGraph, sharedMaterializationCache, sharedDepTypes),
    ] as [string, DependenciesHierarchy]
  }))
  for (const [projectPath, dependenciesHierarchy] of pairs) {
    result[projectPath] = dependenciesHierarchy
  }
  return result
}

async function dependenciesHierarchyForPackage (
  projectPath: string,
  currentLockfile: LockfileObject,
  wantedLockfile: LockfileObject | null,
  opts: {
    depth: number
    excludePeerDependencies?: boolean
    include: { [dependenciesField in DependenciesField]: boolean }
    registries: Registries
    onlyProjects?: boolean
    search?: Finder
    showDedupedSearchMatches?: boolean
    skipped: Set<string>
    lockfileDir: string
    checkWantedLockfileOnly?: boolean
    modulesDir?: string
    virtualStoreDir?: string
    virtualStoreDirMaxLength: number
  },
  graph: DependencyGraph,
  materializationCache: MaterializationCache,
  depTypes: DepTypes
): Promise<DependenciesHierarchy> {
  const importerId = getLockfileImporterId(opts.lockfileDir, projectPath)

  if (!currentLockfile.importers[importerId]) return {}

  const modulesDir = opts.modulesDir && path.isAbsolute(opts.modulesDir)
    ? opts.modulesDir
    : path.join(projectPath, opts.modulesDir ?? 'node_modules')

  const savedDeps = getAllDirectDependencies(currentLockfile.importers[importerId])
  // When searching, unsaved deps are irrelevant — they aren't in the lockfile
  // graph and can't have dependency subtrees showing paths to the search target.
  const unsavedDeps = opts.search
    ? []
    : ((await readModulesDir(modulesDir)) ?? []).filter((directDep) => !savedDeps[directDep])
  const currentPackages = currentLockfile.packages ?? {}
  const wantedPackages = wantedLockfile?.packages ?? {}
  const getTreeOpts = {
    currentPackages,
    excludePeerDependencies: opts.excludePeerDependencies,
    importers: currentLockfile.importers,
    include: opts.include,
    depTypes,
    lockfileDir: opts.lockfileDir,
    onlyProjects: opts.onlyProjects,
    rewriteLinkVersionDir: projectPath,
    maxDepth: opts.depth,
    registries: opts.registries,
    search: opts.search,
    showDedupedSearchMatches: opts.showDedupedSearchMatches,
    skipped: opts.skipped,
    wantedPackages,
    virtualStoreDir: opts.virtualStoreDir,
    virtualStoreDirMaxLength: opts.virtualStoreDirMaxLength,
    modulesDir,
  }
  const parentId: TreeNodeId = { type: 'importer', importerId }

  const getChildrenTree = (nodeId: TreeNodeId, parentDir?: string) =>
    getTree({ ...getTreeOpts, parentDir, graph, materializationCache }, nodeId)
  const result: DependenciesHierarchy = {}
  for (const dependenciesField of DEPENDENCIES_FIELDS.sort().filter(dependenciesField => opts.include[dependenciesField])) {
    const topDeps = currentLockfile.importers[importerId][dependenciesField] ?? {}
    result[dependenciesField] = []
    for (const alias in topDeps) {
      const ref = topDeps[alias]
      const { pkgInfo: packageInfo, readManifest } = getPkgInfo({
        alias,
        currentPackages: currentLockfile.packages ?? {},
        depTypes,
        rewriteLinkVersionDir: projectPath,
        linkedPathBaseDir: projectPath,
        ref,
        registries: opts.registries,
        skipped: opts.skipped,
        wantedPackages: wantedLockfile?.packages ?? {},
        virtualStoreDir: opts.virtualStoreDir,
        virtualStoreDirMaxLength: opts.virtualStoreDirMaxLength,
        modulesDir,
      })
      let newEntry: PackageNode | null = null
      const searchMatch = opts.search?.({
        alias,
        name: packageInfo.name,
        version: packageInfo.version,
        readManifest,
      })
      const nodeId = getTreeNodeChildId({
        parentId,
        dep: { alias, ref },
        lockfileDir: opts.lockfileDir,
        importers: currentLockfile.importers,
      })
      if (opts.onlyProjects && nodeId?.type !== 'importer') {
        continue
      }
      let treeResult: GetTreeResult | undefined
      if (nodeId == null) {
        if ((opts.search != null) && !searchMatch) continue
        newEntry = packageInfo
      } else {
        treeResult = getChildrenTree(nodeId, packageInfo.path)
        if (treeResult.deduped) {
          // This subtree was already materialized for a previous importer.
          const showDeduped = opts.search == null || Boolean(searchMatch) ||
            Boolean(opts.showDedupedSearchMatches && treeResult.hasSearchMatch)
          if (showDeduped) {
            newEntry = packageInfo
            if (treeResult.count > 0) {
              newEntry.deduped = true
              newEntry.dedupedDependenciesCount = treeResult.count
            }
          }
        } else if (treeResult.nodes.length > 0) {
          newEntry = {
            ...packageInfo,
            dependencies: treeResult.nodes,
          }
        } else if ((opts.search == null) || searchMatch) {
          newEntry = packageInfo
        }
      }
      if (newEntry != null) {
        if (nodeId?.type === 'package') {
          const { peerDepGraphHash } = parseDepPath(nodeId.depPath)
          if (peerDepGraphHash) {
            newEntry.peersSuffixHash = crypto.createHash('md5').update(peerDepGraphHash).digest('hex').slice(0, 4)
          }
        }
        if (searchMatch) {
          newEntry.searched = true
          if (typeof searchMatch === 'string') {
            newEntry.searchMessage = searchMatch
          }
        } else if (newEntry.deduped && opts.showDedupedSearchMatches && treeResult?.hasSearchMatch) {
          newEntry.searched = true
          if (treeResult.searchMessages.length > 0) {
            newEntry.searchMessage = treeResult.searchMessages.join('\n')
          }
        }
        result[dependenciesField]!.push(newEntry)
      }
    }
  }

  if (unsavedDeps.length > 0) await Promise.all(
    unsavedDeps.map(async (unsavedDep) => {
      let pkgPath = path.join(modulesDir, unsavedDep)
      let version!: string
      try {
        pkgPath = await resolveLinkTarget(pkgPath)
        version = `link:${normalizePath(path.relative(projectPath, pkgPath))}`
      } catch {
        // if error happened. The package is not a link
        const pkg = await safeReadPackageJsonFromDir(pkgPath)
        version = pkg?.version ?? 'undefined'
      }
      const pkg = {
        alias: unsavedDep,
        isMissing: false,
        isPeer: false,
        isSkipped: false,
        name: unsavedDep,
        path: pkgPath,
        version,
      }
      const searchMatch = opts.search?.({
        alias: pkg.alias,
        name: pkg.name,
        version: pkg.version,
        readManifest: () => readPackageJsonFromDirSync(pkgPath),
      })
      if ((opts.search != null) && !searchMatch) return
      const newEntry: PackageNode = pkg
      if (searchMatch) {
        newEntry.searched = true
        if (typeof searchMatch === 'string') {
          newEntry.searchMessage = searchMatch
        }
      }
      result.unsavedDependencies = result.unsavedDependencies ?? []
      result.unsavedDependencies.push(newEntry)
    })
  )

  return result
}

/**
 * Given the shared dependency graph and a list of search query strings,
 * finds which importers can transitively reach a package whose alias
 * matches any of the queries.
 *
 * Returns the set of importer IDs that should be processed.
 */
function findImportersReachingSearchTarget (
  graph: DependencyGraph,
  fastSearch: (alias: string) => boolean
): Set<string> {
  // 1. Build reverse edges and find nodes whose alias matches the query.
  const reverseEdges = new Map<string, Set<string>>()
  const matchingParentIds = new Set<string>()

  for (const [parentId, graphNode] of graph.nodes) {
    for (const edge of graphNode.edges) {
      if (edge.target != null) {
        let parents = reverseEdges.get(edge.target.id)
        if (parents == null) {
          parents = new Set()
          reverseEdges.set(edge.target.id, parents)
        }
        parents.add(parentId)
      }
      if (fastSearch(edge.alias)) {
        matchingParentIds.add(parentId)
      }
    }
  }

  // 2. BFS backward from matching parent nodes to find reachable importers.
  const visited = new Set<string>()
  const queue = [...matchingParentIds]
  let queueIdx = 0
  const reachableImporterIds = new Set<string>()

  while (queueIdx < queue.length) {
    const nodeId = queue[queueIdx++]
    if (visited.has(nodeId)) continue
    visited.add(nodeId)

    const graphNode = graph.nodes.get(nodeId)
    if (graphNode?.nodeId.type === 'importer') {
      reachableImporterIds.add(graphNode.nodeId.importerId)
    }

    const parents = reverseEdges.get(nodeId)
    if (parents != null) {
      for (const parent of parents) {
        if (!visited.has(parent)) {
          queue.push(parent)
        }
      }
    }
  }

  return reachableImporterIds
}

function getAllDirectDependencies (projectSnapshot: ProjectSnapshot): ResolvedDependencies {
  return {
    ...projectSnapshot.dependencies,
    ...projectSnapshot.devDependencies,
    ...projectSnapshot.optionalDependencies,
  }
}
