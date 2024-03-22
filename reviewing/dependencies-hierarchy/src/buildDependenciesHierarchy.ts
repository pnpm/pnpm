import path from 'node:path'

import normalizePath from 'normalize-path'
import realpathMissing from 'realpath-missing'
import resolveLinkTarget from 'resolve-link-target'

import {
  type Lockfile,
  type Registries,
  type TreeNodeId,
  type PackageNode,
  type PackageInfo,
  DEPENDENCIES_FIELDS,
  type SearchFunction,
  type ProjectSnapshot,
  type DependenciesField,
  type DependenciesHierarchy,
} from '@pnpm/types'
import {
  readWantedLockfile,
  readCurrentLockfile,
  getLockfileImporterId,
} from '@pnpm/lockfile-file'
import { readModulesDir } from '@pnpm/read-modules-dir'
import { readModulesManifest } from '@pnpm/modules-yaml'
import { normalizeRegistries } from '@pnpm/normalize-registries'
import { safeReadPackageJsonFromDir } from '@pnpm/read-package-json'

import { getTree } from './getTree.js'
import { getPkgInfo } from './getPkgInfo.js'
import { getTreeNodeChildId } from './getTreeNodeChildId.js'

export async function buildDependenciesHierarchy(
  projectPaths: string[] | undefined,
  maybeOpts: {
    depth: number
    include?: { [dependenciesField in DependenciesField]: boolean } | undefined
    registries?: Registries | undefined
    onlyProjects?: boolean | undefined
    search?: SearchFunction | undefined
    lockfileDir: string
    modulesDir?: string | undefined
  }
): Promise<Record<string, DependenciesHierarchy>> {
  if (!maybeOpts?.lockfileDir) {
    throw new TypeError('opts.lockfileDir is required')
  }

  const modulesDir = await realpathMissing(
    path.join(maybeOpts.lockfileDir, maybeOpts.modulesDir ?? 'node_modules')
  )

  const modules = await readModulesManifest(modulesDir)

  const registries = normalizeRegistries({
    ...maybeOpts?.registries,
    ...modules?.registries,
  })

  const currentLockfile =
    (modules?.virtualStoreDir &&
      (await readCurrentLockfile(modules.virtualStoreDir, {
        ignoreIncompatible: false,
      }))) ??
    null

  const wantedLockfile = await readWantedLockfile(maybeOpts.lockfileDir, {
    ignoreIncompatible: false,
  })

  if (projectPaths == null) {
    projectPaths = Object.keys(wantedLockfile?.importers ?? {}).map((id) =>
      path.join(maybeOpts.lockfileDir, id)
    )
  }

  const result: Record<string, DependenciesHierarchy> = {}

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
  }

  ;(
    await Promise.all(
      projectPaths.map(async (projectPath: string): Promise<[string, DependenciesHierarchy]> => {
        return [
          projectPath,
          await dependenciesHierarchyForPackage(
            projectPath,
            currentLockfile,
            wantedLockfile,
            opts
          ),
        ]
      })
    )
  ).forEach(([projectPath, dependenciesHierarchy]: [string, DependenciesHierarchy]): void => {
    result[projectPath] = dependenciesHierarchy
  })

  return result
}

async function dependenciesHierarchyForPackage(
  projectPath: string,
  currentLockfile: Lockfile,
  wantedLockfile: Lockfile | null,
  opts: {
    depth: number
    include: { [dependenciesField in DependenciesField]: boolean }
    registries: Registries
    onlyProjects?: boolean | undefined
    search?: SearchFunction | undefined
    skipped: Set<string>
    lockfileDir: string
    modulesDir?: string | undefined
    virtualStoreDir?: string | undefined
  }
): Promise<DependenciesHierarchy> {
  const importerId = getLockfileImporterId(opts.lockfileDir, projectPath)

  if (!currentLockfile.importers[importerId]) {
    return {}
  }

  const modulesDir = path.join(projectPath, opts.modulesDir ?? 'node_modules')

  const savedDeps = getAllDirectDependencies(
    currentLockfile.importers[importerId]
  )

  const allDirectDeps = (await readModulesDir(modulesDir)) ?? []

  const unsavedDeps = allDirectDeps.filter((directDep) => !savedDeps[directDep])

  const getChildrenTree = getTree.bind(null, {
    currentPackages: currentLockfile.packages ?? {},
    importers: currentLockfile.importers,
    includeOptionalDependencies: opts.include.optionalDependencies,
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
  })

  const parentId: TreeNodeId = { type: 'importer', importerId }

  const result: DependenciesHierarchy = {}

  for (const dependenciesField of DEPENDENCIES_FIELDS.sort().filter(
    (dependenciesField: 'optionalDependencies' | 'dependencies' | 'devDependencies'): boolean => {
      return opts.include[dependenciesField];
    }
  )) {
    const topDeps =
      currentLockfile.importers[importerId]?.[dependenciesField] ?? {}

    result[dependenciesField] = []

    Object.entries(topDeps).forEach(([alias, ref]) => {
      const packageInfo = getPkgInfo({
        alias,
        currentPackages: currentLockfile.packages ?? {},
        rewriteLinkVersionDir: projectPath,
        linkedPathBaseDir: projectPath,
        ref,
        registries: opts.registries,
        skipped: opts.skipped,
        wantedPackages: wantedLockfile?.packages ?? {},
        virtualStoreDir: opts.virtualStoreDir,
      })

      let newEntry: (PackageInfo | PackageNode) | null = null

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
        if (opts.search != null && !matchedSearched) {
          return
        }

        newEntry = packageInfo
      } else {
        const dependencies: (PackageNode | PackageInfo)[] = getChildrenTree(nodeId)

        if (dependencies.length > 0) {
          newEntry = {
            ...packageInfo,
            dependencies,
          }
        } else if (opts.search == null || matchedSearched) {
          newEntry = packageInfo
        }
      }

      if (newEntry !== null) {
        if (matchedSearched) {
          newEntry.searched = true
        }

        result[dependenciesField]?.push(newEntry)
      }
    })
  }

  await Promise.all(
    unsavedDeps.map(async (unsavedDep: string): Promise<void> => {
      let pkgPath = path.join(modulesDir, unsavedDep)

      let version: string | undefined

      try {
        pkgPath = await resolveLinkTarget(pkgPath)

        version = `link:${normalizePath(path.relative(projectPath, pkgPath))}`
      } catch (err: any) { // eslint-disable-line
        // if error happened. The package is not a link
        const pkg = await safeReadPackageJsonFromDir(pkgPath)

        version = pkg?.version ?? 'undefined'
      }

      const pkg: PackageNode = {
        alias: unsavedDep,
        isMissing: false,
        isPeer: false,
        isSkipped: false,
        name: unsavedDep,
        path: pkgPath,
        version,
      }

      const matchedSearched = opts.search?.(pkg)

      if (opts.search != null && !matchedSearched) {
        return
      }

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

function getAllDirectDependencies(projectSnapshot: ProjectSnapshot | undefined): Record<string, string> {
  return {
    ...projectSnapshot?.dependencies,
    ...projectSnapshot?.devDependencies,
    ...projectSnapshot?.optionalDependencies,
  }
}
