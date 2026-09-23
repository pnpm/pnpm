import path from 'node:path'

import { normalizeRegistriesByScope } from '@pnpm/config.normalize-registries'
import { readModulesDir } from '@pnpm/fs.read-modules-dir'
import { readModulesManifest } from '@pnpm/installing.modules-yaml'
import { detectDepTypes } from '@pnpm/lockfile.detect-dep-types'
import {
  getLockfileImporterId,
  type LockfileObject,
  type ProjectSnapshot,
  readCurrentLockfile,
  readWantedLockfile,
  type ResolvedDependencies,
} from '@pnpm/lockfile.fs'
import { safeReadPackageJsonFromDir } from '@pnpm/pkg-manifest.reader'
import { StoreIndex } from '@pnpm/store.index'
import { DEPENDENCIES_FIELDS, type DependenciesField, type Finder, type RegistriesByScope } from '@pnpm/types'
import normalizePath from 'normalize-path'
import pLimit from 'p-limit'
import { pathAbsolute } from 'path-absolute'
import { realpathMissing } from 'realpath-missing'
import { resolveLinkTarget } from 'resolve-link-target'

import { buildDependencyGraph } from './buildDependencyGraph.js'
import type { DependencyNode } from './DependencyNode.js'
import { type BaseTreeOpts, getTree, type MaterializationCache } from './getTree.js'
import type { TreeNodeId } from './TreeNodeId.js'

const limitUnsavedReads = pLimit(4)

export interface DependenciesTree {
  dependencies?: DependencyNode[]
  devDependencies?: DependencyNode[]
  optionalDependencies?: DependencyNode[]
  unsavedDependencies?: DependencyNode[]
}

export interface BuildDependenciesTreeOptions {
  depth: number
  excludePeerDependencies?: boolean
  include?: { [dependenciesField in DependenciesField]: boolean }
  registriesByScope?: RegistriesByScope
  registriesByPrefix?: Record<string, string>
  onlyProjects?: boolean
  /**
   * The workspace projects that `onlyProjects` follows through their own
   * lockfiles when the lockfile being read has no importer for them.
   */
  workspaceProjectDirs?: string[]
  search?: Finder
  showDedupedSearchMatches?: boolean
  lockfileDir: string
  checkWantedLockfileOnly?: boolean
  modulesDir?: string
  virtualStoreDirMaxLength: number
}

export async function buildDependenciesTree (
  projectPaths: string[] | undefined,
  maybeOpts: BuildDependenciesTreeOptions
): Promise<{ [projectDir: string]: DependenciesTree }> {
  return buildProjectsTrees(projectPaths, maybeOpts, { ancestors: new Set(), expanded: new Map() })
}

interface LinkedProjectsWalk {
  /** The linked projects whose trees enclose the current one. */
  ancestors: Set<string>
  /**
   * The number of dependencies under each linked project already expanded in
   * the output, by path and depth.
   */
  expanded: Map<string, number>
}

async function buildProjectsTrees (
  projectPaths: string[] | undefined,
  maybeOpts: BuildDependenciesTreeOptions,
  linkedWalk: LinkedProjectsWalk
): Promise<{ [projectDir: string]: DependenciesTree }> {
  if (!maybeOpts?.lockfileDir) {
    throw new TypeError('opts.lockfileDir is required')
  }
  const modulesDir = await realpathMissing(pathAbsolute(maybeOpts.modulesDir ?? 'node_modules', maybeOpts.lockfileDir))
  const modules = await readModulesManifest(modulesDir)
  const registriesByScope = normalizeRegistriesByScope(maybeOpts?.registriesByScope)
  const internalPnpmDir = path.join(modulesDir, '.pnpm')
  const currentLockfile = await readCurrentLockfile(internalPnpmDir, { ignoreIncompatible: false })
  const wantedLockfile = await readWantedLockfile(maybeOpts.lockfileDir, { ignoreIncompatible: false })
  if (projectPaths == null) {
    projectPaths = Object.keys(wantedLockfile?.importers ?? {})
      .map((id) => path.join(maybeOpts.lockfileDir, id))
  }

  const result = {} as { [projectDir: string]: DependenciesTree }

  const lockfileToUse = maybeOpts.checkWantedLockfileOnly ? wantedLockfile : (currentLockfile ?? wantedLockfile)

  if (!lockfileToUse) {
    for (const projectPath of projectPaths) {
      result[projectPath] = {}
    }
    return result
  }

  const storeDir = modules?.storeDir
  const storeIndex = storeDir ? new StoreIndex(storeDir) : undefined
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
    registriesByScope,
    registriesByPrefix: maybeOpts.registriesByPrefix,
    search: maybeOpts.search,
    showDedupedSearchMatches: maybeOpts.showDedupedSearchMatches ?? (maybeOpts.search != null),
    skipped: new Set(modules?.skipped ?? []),
    storeDir,
    storeIndex,
    modulesDir,
    virtualStoreDir: modules?.virtualStoreDir,
    virtualStoreDirMaxLength: modules?.virtualStoreDirMaxLength ?? maybeOpts.virtualStoreDirMaxLength,
  }
  // Build the dependency graph ONCE for all importers and share a single
  // MaterializationCache so that identical subtrees are only materialized once.
  const allRootIds: TreeNodeId[] = []
  for (const projectPath of projectPaths) {
    const importerId = getLockfileImporterId(opts.lockfileDir, projectPath)
    if (lockfileToUse.importers[importerId]) {
      allRootIds.push({ type: 'importer', importerId })
    }
  }
  const sharedGraph = buildDependencyGraph(allRootIds, {
    currentPackages: lockfileToUse.packages ?? {},
    importers: lockfileToUse.importers,
    include: opts.include,
    lockfileDir: opts.lockfileDir,
    onlyProjects: opts.onlyProjects,
  })
  const sharedMaterializationCache: MaterializationCache = new Map()
  const sharedDepTypes = detectDepTypes(lockfileToUse)

  const ctx: HierarchyContext = {
    currentLockfile: lockfileToUse,
    wantedLockfile,
    ...opts,
    graph: sharedGraph,
    materializationCache: sharedMaterializationCache,
    depTypes: sharedDepTypes,
  }

  const getHierarchy = dependenciesHierarchyForPackage.bind(null, ctx)

  const pairs = await Promise.all(projectPaths.map(async (projectPath) => {
    return [
      projectPath,
      await getHierarchy(projectPath),
    ] as [string, DependenciesTree]
  }))
  storeIndex?.close()
  for (const [projectPath, dependenciesHierarchy] of pairs) {
    result[projectPath] = dependenciesHierarchy
  }
  if (opts.onlyProjects) {
    // Sequential, so that the first occurrence of a linked project is the
    // one expanded, as with the deduplication of a shared lockfile.
    for (const [projectPath, dependenciesHierarchy] of pairs) {
      // eslint-disable-next-line no-await-in-loop
      await expandLinkedProjects(dependenciesHierarchy, {
        importers: lockfileToUse.importers,
        lockfileDir: opts.lockfileDir,
        depth: opts.depth,
        treeOpts: maybeOpts,
        workspaceProjectDirs: new Set(maybeOpts.workspaceProjectDirs),
        walk: { ...linkedWalk, ancestors: new Set([...linkedWalk.ancestors, projectPath]) },
        rewriteLinkVersionDir: projectPath,
      })
    }
  }
  return result
}

interface LinkedProjectsContext {
  importers: Record<string, ProjectSnapshot>
  lockfileDir: string
  depth: number
  treeOpts: BuildDependenciesTreeOptions
  workspaceProjectDirs: Set<string>
  walk: LinkedProjectsWalk
  rewriteLinkVersionDir: string
}

/**
 * Attaches the project dependencies of every linked workspace project that
 * the lockfile has no importer for. With `sharedWorkspaceLockfile: false`, the lockfile
 * that the tree was built from knows nothing about the dependencies of the
 * other workspace projects.
 */
async function expandLinkedProjects (tree: DependenciesTree, ctx: LinkedProjectsContext): Promise<void> {
  for (const field of DEPENDENCIES_FIELDS) {
    if (tree[field] != null) {
      // eslint-disable-next-line no-await-in-loop
      tree[field] = await expandLinkedProjectNodes(tree[field], 0, ctx)
    }
  }
}

async function expandLinkedProjectNodes (
  nodes: DependencyNode[],
  level: number,
  ctx: LinkedProjectsContext
): Promise<DependencyNode[]> {
  const expanded: DependencyNode[] = []
  for (const node of nodes) {
    let expandedNode: DependencyNode | undefined = node
    if (node.dependencies != null) {
      // eslint-disable-next-line no-await-in-loop
      expandedNode = keepSearched({ ...node, dependencies: await expandLinkedProjectNodes(node.dependencies, level + 1, ctx) }, ctx)
    } else if (!node.circular && ctx.importers[getLockfileImporterId(ctx.lockfileDir, node.path)] == null) {
      // eslint-disable-next-line no-await-in-loop
      expandedNode = await expandLinkedProject(node, level, ctx)
    }
    if (expandedNode != null) expanded.push(expandedNode)
  }
  return expanded
}

async function expandLinkedProject (
  node: DependencyNode,
  level: number,
  ctx: LinkedProjectsContext
): Promise<DependencyNode | undefined> {
  if (!ctx.workspaceProjectDirs.has(node.path)) return undefined
  if (ctx.walk.ancestors.has(node.path)) return keepSearched({ ...node, circular: true }, ctx)
  if (level >= ctx.depth) return keepSearched(node, ctx)
  const depth = ctx.depth - level - 1
  const key = `${node.path}@${depth}`
  const previousCount = ctx.walk.expanded.get(key)
  if (previousCount != null) {
    return previousCount > 0
      ? { ...node, deduped: true, dedupedDependenciesCount: previousCount }
      : keepSearched(node, ctx)
  }
  const linkedTrees = await buildProjectsTrees([node.path], {
    ...ctx.treeOpts,
    lockfileDir: node.path,
    depth,
  }, ctx.walk)
  const linkedTree = linkedTrees[node.path]
  const dependencies = DEPENDENCIES_FIELDS.flatMap((field) => linkedTree[field] ?? [])
  ctx.walk.expanded.set(key, countNodes(dependencies))
  return keepSearched(dependencies.length > 0
    ? { ...node, dependencies: rewriteLinkVersions(dependencies, ctx.rewriteLinkVersionDir) }
    : node, ctx)
}

function countNodes (nodes: DependencyNode[]): number {
  return nodes.reduce((count, node) => count + 1 + countNodes(node.dependencies ?? []), 0)
}

function keepSearched (node: DependencyNode, ctx: LinkedProjectsContext): DependencyNode | undefined {
  return ctx.treeOpts.search == null || node.searched || node.dependencies?.length ? node : undefined
}

function rewriteLinkVersions (nodes: DependencyNode[], rewriteLinkVersionDir: string): DependencyNode[] {
  return nodes.map((node) => ({
    ...node,
    version: node.version.startsWith('link:')
      ? `link:${normalizePath(path.relative(rewriteLinkVersionDir, node.path))}`
      : node.version,
    ...(node.dependencies && { dependencies: rewriteLinkVersions(node.dependencies, rewriteLinkVersionDir) }),
  }))
}

interface HierarchyContext extends BaseTreeOpts {
  currentLockfile: LockfileObject
  wantedLockfile: LockfileObject | null
  depth: number
  checkWantedLockfileOnly?: boolean
}

async function dependenciesHierarchyForPackage (
  opts: HierarchyContext,
  projectPath: string
): Promise<DependenciesTree> {
  const { currentLockfile, wantedLockfile } = opts
  const importerId = getLockfileImporterId(opts.lockfileDir, projectPath)

  if (!currentLockfile.importers[importerId]) return {}

  const modulesDir = opts.modulesDir && path.isAbsolute(opts.modulesDir)
    ? opts.modulesDir
    : path.join(projectPath, opts.modulesDir ?? 'node_modules')

  const currentPackages = currentLockfile.packages ?? {}
  const wantedPackages = wantedLockfile?.packages ?? {}

  // Build a map from alias → dependency field for post-categorization.
  const result: DependenciesTree = {}
  const fieldMap = new Map<string, DependenciesField>()
  for (const field of DEPENDENCIES_FIELDS.sort().filter(f => opts.include[f])) {
    result[field] = []
    const fieldDeps = currentLockfile.importers[importerId][field] ?? {}
    for (const alias in fieldDeps) {
      fieldMap.set(alias, field)
    }
  }

  const parentId: TreeNodeId = { type: 'importer', importerId }

  // Materialize the tree rooted at this importer in a single getTree call.
  // materializeChildren handles all dedup, search, and circular detection.
  // The depth is incremented by 1 because the importer itself is one level;
  // opts.depth controls how deep *below* the direct dependencies we go.
  const nodes = getTree({
    ...opts,
    currentPackages,
    importers: currentLockfile.importers,
    rewriteLinkVersionDir: projectPath,
    maxDepth: opts.depth + 1,
    wantedPackages,
    modulesDir,
  }, parentId)

  // Categorize the materialized nodes into their dependency fields.
  for (const node of nodes) {
    const field = fieldMap.get(node.alias)
    if (field != null) {
      result[field]!.push(node)
    }
  }

  // Handle unsaved dependencies (packages in node_modules but not in lockfile).
  // When searching, unsaved deps are irrelevant — they aren't in the lockfile
  // graph and can't have dependency subtrees showing paths to the search target.
  if (!opts.search) {
    const savedDeps = getAllDirectDependencies(currentLockfile.importers[importerId])
    const unsavedDeps = ((await readModulesDir(modulesDir)) ?? []).filter((directDep) => !savedDeps[directDep])
    if (unsavedDeps.length > 0) await Promise.all(
      unsavedDeps.map((unsavedDep) => limitUnsavedReads(async () => {
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
        const pkg: DependencyNode = {
          alias: unsavedDep,
          isMissing: false,
          isPeer: false,
          isSkipped: false,
          name: unsavedDep,
          path: pkgPath,
          version,
        }
        result.unsavedDependencies = result.unsavedDependencies ?? []
        result.unsavedDependencies.push(pkg)
      }))
    )
  }

  return result
}

function getAllDirectDependencies (projectSnapshot: ProjectSnapshot): ResolvedDependencies {
  return {
    ...projectSnapshot.dependencies,
    ...projectSnapshot.devDependencies,
    ...projectSnapshot.optionalDependencies,
  }
}
