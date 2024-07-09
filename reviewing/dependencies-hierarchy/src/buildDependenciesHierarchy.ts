import path from 'path'
import {
  getLockfileImporterId,
  type Lockfile,
  type ProjectSnapshot,
  readCurrentLockfile,
  readWantedLockfile,
  type ResolvedDependencies,
} from '@pnpm/lockfile-file'
import { detectDepTypes } from '@pnpm/lockfile.detect-dep-types'
import { readModulesManifest } from '@pnpm/modules-yaml'
import { normalizeRegistries } from '@pnpm/normalize-registries'
import { readModulesDir } from '@pnpm/read-modules-dir'
import { safeReadPackageJsonFromDir } from '@pnpm/read-package-json'
import { type DependenciesField, DEPENDENCIES_FIELDS, type Registries } from '@pnpm/types'
import normalizePath from 'normalize-path'
import realpathMissing from 'realpath-missing'
import resolveLinkTarget from 'resolve-link-target'
import { type PackageNode } from './PackageNode'
import { type SearchFunction } from './types'
import { getTree } from './getTree'
import { getTreeNodeChildId } from './getTreeNodeChildId'
import { getPkgInfo } from './getPkgInfo'
import { type TreeNodeId } from './TreeNodeId'

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
    include?: { [dependenciesField in DependenciesField]: boolean }
    registries?: Registries
    onlyProjects?: boolean
    search?: SearchFunction
    lockfileDir: string
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
  const currentLockfile = (modules?.virtualStoreDir && await readCurrentLockfile(modules.virtualStoreDir, { ignoreIncompatible: false })) ?? null
  const wantedLockfile = await readWantedLockfile(maybeOpts.lockfileDir, { ignoreIncompatible: false })
  if (projectPaths == null) {
    projectPaths = Object.keys(wantedLockfile?.importers ?? {})
      .map((id) => path.join(maybeOpts.lockfileDir, id))
  }

  const result = {} as { [projectDir: string]: DependenciesHierarchy }

  if (!currentLockfile) {
    for (const projectPath of projectPaths) {
      result[projectPath] = {}
    }
    return result
  }

  const opts = {
    depth: maybeOpts.depth || 0,
    include: maybeOpts.include ?? {
      dependencies: true,
      devDependencies: true,
      optionalDependencies: true,
    },
    lockfileDir: maybeOpts.lockfileDir,
    onlyProjects: maybeOpts.onlyProjects,
    registries,
    search: maybeOpts.search,
    skipped: new Set(modules?.skipped ?? []),
    modulesDir: maybeOpts.modulesDir,
    virtualStoreDir: modules?.virtualStoreDir,
    virtualStoreDirMaxLength: modules?.virtualStoreDirMaxLength ?? maybeOpts.virtualStoreDirMaxLength,
  }
  ; (
    await Promise.all(projectPaths.map(async (projectPath) => {
      return [
        projectPath,
        await dependenciesHierarchyForPackage(projectPath, currentLockfile, wantedLockfile, opts),
      ] as [string, DependenciesHierarchy]
    }))
  ).forEach(([projectPath, dependenciesHierarchy]) => {
    result[projectPath] = dependenciesHierarchy
  })
  return result
}

async function dependenciesHierarchyForPackage (
  projectPath: string,
  currentLockfile: Lockfile,
  wantedLockfile: Lockfile | null,
  opts: {
    depth: number
    include: { [dependenciesField in DependenciesField]: boolean }
    registries: Registries
    onlyProjects?: boolean
    search?: SearchFunction
    skipped: Set<string>
    lockfileDir: string
    modulesDir?: string
    virtualStoreDir?: string
    virtualStoreDirMaxLength: number
  }
): Promise<DependenciesHierarchy> {
  const importerId = getLockfileImporterId(opts.lockfileDir, projectPath)

  if (!currentLockfile.importers[importerId]) return {}

  const modulesDir = path.join(projectPath, opts.modulesDir ?? 'node_modules')

  const savedDeps = getAllDirectDependencies(currentLockfile.importers[importerId])
  const allDirectDeps = await readModulesDir(modulesDir) ?? []
  const unsavedDeps = allDirectDeps.filter((directDep) => !savedDeps[directDep])

  const depTypes = detectDepTypes(currentLockfile)
  const getChildrenTree = getTree.bind(null, {
    currentPackages: currentLockfile.packages ?? {},
    importers: currentLockfile.importers,
    includeOptionalDependencies: opts.include.optionalDependencies,
    depTypes,
    lockfileDir: opts.lockfileDir,
    onlyProjects: opts.onlyProjects,
    rewriteLinkVersionDir: projectPath,
    maxDepth: opts.depth,
    modulesDir,
    registries: opts.registries,
    search: opts.search,
    skipped: opts.skipped,
    wantedPackages: wantedLockfile?.packages ?? {},
    virtualStoreDir: opts.virtualStoreDir,
    virtualStoreDirMaxLength: opts.virtualStoreDirMaxLength,
  })
  const parentId: TreeNodeId = { type: 'importer', importerId }
  const result: DependenciesHierarchy = {}
  for (const dependenciesField of DEPENDENCIES_FIELDS.sort().filter(dependenciesField => opts.include[dependenciesField])) {
    const topDeps = currentLockfile.importers[importerId][dependenciesField] ?? {}
    result[dependenciesField] = []
    Object.entries(topDeps).forEach(([alias, ref]) => {
      const packageInfo = getPkgInfo({
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
      })
      let newEntry: PackageNode | null = null
      const matchedSearched = opts.search?.(packageInfo)
      const nodeId = getTreeNodeChildId({
        parentId,
        dep: { alias, ref },
        lockfileDir: opts.lockfileDir,
        importers: currentLockfile.importers,
      })
      if (opts.onlyProjects && nodeId?.type !== 'importer') {
        return
      } else if (nodeId == null) {
        if ((opts.search != null) && !matchedSearched) return
        newEntry = packageInfo
      } else {
        const dependencies = getChildrenTree(nodeId)
        if (dependencies.length > 0) {
          newEntry = {
            ...packageInfo,
            dependencies,
          }
        } else if ((opts.search == null) || matchedSearched) {
          newEntry = packageInfo
        }
      }
      if (newEntry != null) {
        if (matchedSearched) {
          newEntry.searched = true
        }
        result[dependenciesField]!.push(newEntry)
      }
    })
  }

  await Promise.all(
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
      const matchedSearched = opts.search?.(pkg)
      if ((opts.search != null) && !matchedSearched) return
      const newEntry: PackageNode = pkg
      if (matchedSearched) {
        newEntry.searched = true
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
