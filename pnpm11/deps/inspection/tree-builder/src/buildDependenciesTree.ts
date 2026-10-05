import path from 'node:path'

import { normalizeRegistriesByScope } from '@pnpm/config.normalize-registries'
import { withCollapsedVariants } from '@pnpm/deps.path'
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
import { getPeerSatisfactionEdgesToSkip } from '@pnpm/lockfile.peer-edges'
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
  /**
   * The workspace projects that dependents link through their publish
   * directory (`publishConfig.directory`), keyed by that directory.
   */
  workspaceProjectPublishDirs?: Record<string, string>
  search?: Finder
  showDedupedSearchMatches?: boolean
  lockfileDir: string
  checkWantedLockfileOnly?: boolean
  modulesDir?: string
  resolvePeersFromWorkspaceRoot?: boolean
  virtualStoreDirMaxLength: number
  nodeLinker?: 'hoisted' | 'isolated' | 'pnp'
}

export async function buildDependenciesTree (
  projectPaths: string[] | undefined,
  maybeOpts: BuildDependenciesTreeOptions
): Promise<{ [projectDir: string]: DependenciesTree }> {
  return buildProjectsTrees(projectPaths, maybeOpts, {
    ancestors: new Set(),
    expanded: new Map(),
    linkedProjectDirs: new Map([
      ...Object.entries(maybeOpts.workspaceProjectPublishDirs ?? {}),
      ...(maybeOpts.workspaceProjectDirs ?? []).map((dir) => [dir, dir] as const),
    ]),
  })
}

interface LinkedProjectsWalk {
  ancestors: Set<string>
  /**
   * A linked project met again at the same depth is marked deduped instead of
   * walked again.
   */
  expanded: Map<string, number>
  /**
   * The workspace project each directory that dependents link to belongs to.
   */
  linkedProjectDirs: ReadonlyMap<string, string>
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
  const { currentLockfile, wantedLockfile } = await readLockfiles(modulesDir, maybeOpts.lockfileDir)
  const projectDirs = projectPaths ?? getImporterDirs(wantedLockfile, maybeOpts.lockfileDir)

  const lockfileToUse = maybeOpts.checkWantedLockfileOnly ? wantedLockfile : (currentLockfile ?? wantedLockfile)

  if (!lockfileToUse) {
    return Object.fromEntries(projectDirs.map((projectPath) => [projectPath, {}]))
  }

  const storeIndex = modules?.storeDir ? new StoreIndex(modules.storeDir) : undefined
  const ctx = createHierarchyContext({
    lockfile: lockfileToUse,
    wantedLockfile,
    projectDirs,
    treeOpts: maybeOpts,
    modules,
    modulesDir,
    registriesByScope,
    storeIndex,
  })

  const pairs = await Promise.all(projectDirs.map(async (projectPath) => {
    return [
      projectPath,
      await dependenciesHierarchyForPackage(ctx, projectPath),
    ] as [string, DependenciesTree]
  }))
  storeIndex?.close()
  if (ctx.onlyProjects) {
    await expandLinkedProjectsOfEach(pairs, {
      importers: lockfileToUse.importers,
      lockfileDir: ctx.lockfileDir,
      depth: ctx.depth,
      treeOpts: maybeOpts,
      linkedWalk,
    })
  }
  return Object.fromEntries(pairs)
}

async function readLockfiles (
  modulesDir: string,
  lockfileDir: string
): Promise<{ currentLockfile: LockfileObject | null, wantedLockfile: LockfileObject | null }> {
  const installStateDir = path.join(modulesDir, '.pnpm')
  const currentLockfile = await readCurrentLockfile(installStateDir, { ignoreIncompatible: false })
  const wantedLockfile = await readWantedLockfile(lockfileDir, { ignoreIncompatible: false })
  return { currentLockfile, wantedLockfile }
}

function getImporterDirs (wantedLockfile: LockfileObject | null, lockfileDir: string): string[] {
  return Object.keys(wantedLockfile?.importers ?? {})
    .map((id) => path.join(lockfileDir, id))
}

interface HierarchyContextSources {
  lockfile: LockfileObject
  wantedLockfile: LockfileObject | null
  projectDirs: string[]
  treeOpts: BuildDependenciesTreeOptions
  modules: Awaited<ReturnType<typeof readModulesManifest>>
  modulesDir: string
  registriesByScope: RegistriesByScope
  storeIndex?: StoreIndex
}

function createHierarchyContext (sources: HierarchyContextSources): HierarchyContext {
  const { lockfile, treeOpts } = sources
  const opts = createTreeOptions(sources)
  // Build the dependency graph ONCE for all importers and share a single
  // MaterializationCache so that identical subtrees are only materialized once.
  const allRootIds: TreeNodeId[] = []
  for (const projectPath of sources.projectDirs) {
    const importerId = getLockfileImporterId(opts.lockfileDir, projectPath)
    if (lockfile.importers[importerId]) {
      allRootIds.push({ type: 'importer', importerId })
    }
  }
  const sharedGraph = buildDependencyGraph(allRootIds, {
    currentPackages: lockfile.packages ?? {},
    importers: lockfile.importers,
    include: opts.include,
    lockfileDir: opts.lockfileDir,
    onlyProjects: opts.onlyProjects,
    peerSatisfactionEdges: getPeerSatisfactionEdgesToSkip(lockfile, {
      include: opts.include,
      resolvePeersFromWorkspaceRoot: treeOpts.resolvePeersFromWorkspaceRoot,
    }),
  })
  const sharedMaterializationCache: MaterializationCache = new Map()
  const sharedDepTypes = detectDepTypes(lockfile, treeOpts)

  return {
    currentLockfile: lockfile,
    wantedLockfile: sources.wantedLockfile,
    ...opts,
    graph: sharedGraph,
    materializationCache: sharedMaterializationCache,
    depTypes: sharedDepTypes,
  }
}

function createTreeOptions ({ treeOpts, modules, modulesDir, registriesByScope, storeIndex }: HierarchyContextSources) {
  return {
    depth: treeOpts.depth || 0,
    excludePeerDependencies: treeOpts.excludePeerDependencies,
    include: treeOpts.include ?? {
      dependencies: true,
      devDependencies: true,
      optionalDependencies: true,
    },
    lockfileDir: treeOpts.lockfileDir,
    checkWantedLockfileOnly: treeOpts.checkWantedLockfileOnly,
    onlyProjects: treeOpts.onlyProjects,
    registriesByScope,
    registriesByPrefix: treeOpts.registriesByPrefix,
    search: treeOpts.search,
    showDedupedSearchMatches: treeOpts.showDedupedSearchMatches ?? (treeOpts.search != null),
    skipped: new Set(modules?.skipped ?? []),
    storeDir: modules?.storeDir,
    storeIndex,
    modulesDir,
    virtualStoreDir: modules?.virtualStoreDir,
    virtualStoreDirMaxLength: modules?.virtualStoreDirMaxLength ?? treeOpts.virtualStoreDirMaxLength,
    nodeLinker: modules?.nodeLinker ?? treeOpts.nodeLinker,
    hoistedLocations: modules?.hoistedLocations && withCollapsedVariants(modules.hoistedLocations),
  }
}

async function expandLinkedProjectsOfEach (
  pairs: Array<[string, DependenciesTree]>,
  opts: Omit<LinkedProjectsContext, 'walk' | 'rewriteLinkVersionDir'> & { linkedWalk: LinkedProjectsWalk }
): Promise<void> {
  const { linkedWalk, ...ctx } = opts
  // Sequential, so that the first occurrence of a linked project is the
  // one expanded, as with the deduplication of a shared lockfile.
  for (const [projectPath, dependenciesHierarchy] of pairs) {
    // eslint-disable-next-line no-await-in-loop -- the first occurrence of a linked project must be the one expanded, so projects are expanded in order
    await expandLinkedProjects(dependenciesHierarchy, {
      ...ctx,
      walk: { ...linkedWalk, ancestors: new Set([...linkedWalk.ancestors, projectPath]) },
      rewriteLinkVersionDir: projectPath,
    })
  }
}

interface LinkedProjectsContext {
  importers: Record<string, ProjectSnapshot>
  lockfileDir: string
  depth: number
  treeOpts: BuildDependenciesTreeOptions
  walk: LinkedProjectsWalk
  rewriteLinkVersionDir: string
}

/**
 * Attaches the project dependencies of every linked workspace project that
 * the lockfile has no importer for. With `sharedWorkspaceLockfile: false`,
 * the lockfile that the tree was built from knows nothing about the
 * dependencies of the other workspace projects.
 */
async function expandLinkedProjects (tree: DependenciesTree, ctx: LinkedProjectsContext): Promise<void> {
  for (const field of DEPENDENCIES_FIELDS) {
    if (tree[field] != null) {
      // eslint-disable-next-line no-await-in-loop -- the walk is sequential so the first occurrence of a linked project is the one expanded
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
      // eslint-disable-next-line no-await-in-loop -- the walk is sequential so the first occurrence of a linked project is the one expanded
      expandedNode = keepSearched({ ...node, dependencies: await expandLinkedProjectNodes(node.dependencies, level + 1, ctx) }, ctx)
    } else if (!node.circular && ctx.importers[getLockfileImporterId(ctx.lockfileDir, node.path)] == null) {
      // eslint-disable-next-line no-await-in-loop -- the walk is sequential so the first occurrence of a linked project is the one expanded
      expandedNode = await expandLinkedProject(node, level, ctx)
    }
    if (expandedNode != null) expanded.push(expandedNode)
  }
  return expanded
}

async function expandLinkedProject (
  linkedNode: DependencyNode,
  level: number,
  ctx: LinkedProjectsContext
): Promise<DependencyNode | undefined> {
  const projectDir = ctx.walk.linkedProjectDirs.get(linkedNode.path)
  if (projectDir == null) return undefined
  const node = { ...linkedNode, path: projectDir }
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
    ? { ...node, dependencies: rewriteLinkVersions(dependencies, node.path, ctx.rewriteLinkVersionDir) }
    : node, ctx)
}

function countNodes (nodes: DependencyNode[]): number {
  return nodes.reduce((count, node) => count + 1 + countNodes(node.dependencies ?? []), 0)
}

function keepSearched (node: DependencyNode, ctx: LinkedProjectsContext): DependencyNode | undefined {
  return ctx.treeOpts.search == null || node.searched || node.dependencies?.length ? node : undefined
}

/**
 * Rebases the `link:` versions of a linked project's dependencies, which are
 * relative to `linkedProjectDir`, onto `rewriteLinkVersionDir`.
 */
function rewriteLinkVersions (nodes: DependencyNode[], linkedProjectDir: string, rewriteLinkVersionDir: string): DependencyNode[] {
  return nodes.map((node) => ({
    ...node,
    version: node.version.startsWith('link:')
      ? `link:${normalizePath(path.relative(rewriteLinkVersionDir, path.resolve(linkedProjectDir, node.version.slice('link:'.length))))}`
      : node.version,
    ...(node.dependencies && { dependencies: rewriteLinkVersions(node.dependencies, linkedProjectDir, rewriteLinkVersionDir) }),
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
  const projectSnapshot = currentLockfile.importers[importerId]

  if (!projectSnapshot) return {}

  const modulesDir = opts.modulesDir && path.isAbsolute(opts.modulesDir)
    ? opts.modulesDir
    : path.join(projectPath, opts.modulesDir ?? 'node_modules')

  const { result, fieldMap } = initDependencyFields(projectSnapshot, opts.include)

  // The depth is incremented by 1 because the importer itself is one level;
  // opts.depth controls how deep *below* the direct dependencies we go.
  const nodes = getTree({
    ...opts,
    currentPackages: currentLockfile.packages ?? {},
    importers: currentLockfile.importers,
    rewriteLinkVersionDir: projectPath,
    maxDepth: opts.depth + 1,
    wantedPackages: wantedLockfile?.packages ?? {},
    modulesDir,
  }, { type: 'importer', importerId })

  for (const node of nodes) {
    const field = fieldMap.get(node.alias)
    if (field != null) {
      result[field]!.push(node)
    }
  }

  // Handle unsaved dependencies (packages in node_modules but not in lockfile).
  // When searching, unsaved deps are irrelevant — they aren't in the lockfile
  // graph and can't have dependency subtrees showing paths to the search target.
  // They aren't workspace projects either, which is all onlyProjects lists.
  if (!opts.search && !opts.onlyProjects) {
    await addUnsavedDependencies(result, { modulesDir, projectPath, projectSnapshot })
  }

  return result
}

function initDependencyFields (
  projectSnapshot: ProjectSnapshot,
  include: HierarchyContext['include']
): { result: DependenciesTree, fieldMap: Map<string, DependenciesField> } {
  const result: DependenciesTree = {}
  const fieldMap = new Map<string, DependenciesField>()
  for (const field of DEPENDENCIES_FIELDS.sort().filter(f => include[f])) {
    result[field] = []
    for (const alias in projectSnapshot[field] ?? {}) {
      fieldMap.set(alias, field)
    }
  }
  return { result, fieldMap }
}

interface UnsavedDependenciesLocation {
  modulesDir: string
  projectPath: string
  projectSnapshot: ProjectSnapshot
}

async function addUnsavedDependencies (
  result: DependenciesTree,
  { modulesDir, projectPath, projectSnapshot }: UnsavedDependenciesLocation
): Promise<void> {
  const savedDeps = getAllDirectDependencies(projectSnapshot)
  const unsavedDeps = ((await readModulesDir(modulesDir)) ?? []).filter((directDep) => !Object.hasOwn(savedDeps, directDep))
  if (unsavedDeps.length === 0) return
  await Promise.all(
    unsavedDeps.map((unsavedDep) => limitUnsavedReads(async () => {
      const pkg = await readUnsavedDependency(unsavedDep, { modulesDir, projectPath })
      result.unsavedDependencies = result.unsavedDependencies ?? []
      result.unsavedDependencies.push(pkg)
    }))
  )
}

async function readUnsavedDependency (
  unsavedDep: string,
  { modulesDir, projectPath }: Pick<UnsavedDependenciesLocation, 'modulesDir' | 'projectPath'>
): Promise<DependencyNode> {
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
  return {
    alias: unsavedDep,
    isMissing: false,
    isPeer: false,
    isSkipped: false,
    name: unsavedDep,
    path: pkgPath,
    version,
  }
}

function getAllDirectDependencies (projectSnapshot: ProjectSnapshot): ResolvedDependencies {
  return {
    ...projectSnapshot.dependencies,
    ...projectSnapshot.devDependencies,
    ...projectSnapshot.optionalDependencies,
  }
}
