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
import { safeReadPackageJsonFromDir } from '@pnpm/read-package-json'
import { type DependenciesField, type Finder, DEPENDENCIES_FIELDS, type Registries } from '@pnpm/types'
import normalizePath from 'normalize-path'
import realpathMissing from 'realpath-missing'
import resolveLinkTarget from 'resolve-link-target'
import { type DependencyNode } from './DependencyNode.js'
import { buildDependencyGraph } from './buildDependencyGraph.js'
import { getTree, type BaseTreeOpts, type MaterializationCache } from './getTree.js'
import { type TreeNodeId } from './TreeNodeId.js'

export interface DependenciesTree {
  dependencies?: DependencyNode[]
  devDependencies?: DependencyNode[]
  optionalDependencies?: DependencyNode[]
  unsavedDependencies?: DependencyNode[]
}

export async function buildDependenciesTree (
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
): Promise<{ [projectDir: string]: DependenciesTree }> {
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

  const result = {} as { [projectDir: string]: DependenciesTree }

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
    storeDir: modules?.storeDir,
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
    ] as [string, DependenciesTree]
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
      })
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
