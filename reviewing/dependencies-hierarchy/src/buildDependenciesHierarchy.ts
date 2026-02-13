import path from 'path'
import {
  getLockfileImporterId,
  type LockfileObject,
  type ProjectSnapshot,
  readCurrentLockfile,
  readWantedLockfile,
  type ResolvedDependencies,
} from '@pnpm/lockfile.fs'
import { detectDepTypes } from '@pnpm/lockfile.detect-dep-types'
import { readModulesManifest } from '@pnpm/modules-yaml'
import { normalizeRegistries } from '@pnpm/normalize-registries'
import { readModulesDir } from '@pnpm/read-modules-dir'
import { safeReadPackageJsonFromDir, readPackageJsonFromDirSync } from '@pnpm/read-package-json'
import { type DependenciesField, type Finder, DEPENDENCIES_FIELDS, type Registries } from '@pnpm/types'
import normalizePath from 'normalize-path'
import realpathMissing from 'realpath-missing'
import resolveLinkTarget from 'resolve-link-target'
import { type PackageNode } from './PackageNode.js'
import { buildDependencyGraph } from './buildDependencyGraph.js'
import { getTree, type BaseTreeOpts, type GetTreeResult, type MaterializationCache } from './getTree.js'
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
  const wantedLockfile = await readWantedLockfile(maybeOpts.lockfileDir, { ignoreIncompatible: false })
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
    ] as [string, DependenciesHierarchy]
  }))
  for (const [projectPath, dependenciesHierarchy] of pairs) {
    result[projectPath] = dependenciesHierarchy
  }
  return result
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
): Promise<DependenciesHierarchy> {
  const { currentLockfile, wantedLockfile, graph, materializationCache, depTypes } = opts
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
    ...opts,
    currentPackages,
    importers: currentLockfile.importers,
    rewriteLinkVersionDir: projectPath,
    maxDepth: opts.depth,
    wantedPackages,
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

function getAllDirectDependencies (projectSnapshot: ProjectSnapshot): ResolvedDependencies {
  return {
    ...projectSnapshot.dependencies,
    ...projectSnapshot.devDependencies,
    ...projectSnapshot.optionalDependencies,
  }
}
