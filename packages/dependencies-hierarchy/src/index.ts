import {
  getLockfileImporterId,
  Lockfile,
  PackageSnapshot,
  PackageSnapshots,
  ProjectSnapshot,
  readCurrentLockfile,
  readWantedLockfile,
} from '@pnpm/lockfile-file'
import {
  nameVerFromPkgSnapshot,
  pkgSnapshotToResolution,
} from '@pnpm/lockfile-utils'
import { read as readModulesYaml } from '@pnpm/modules-yaml'
import normalizeRegistries from '@pnpm/normalize-registries'
import pkgIdToFilename from '@pnpm/pkgid-to-filename'
import readModulesDir from '@pnpm/read-modules-dir'
import { safeReadPackageFromDir } from '@pnpm/read-package-json'
import { DependenciesField, DEPENDENCIES_FIELDS, Registries } from '@pnpm/types'
import { refToRelative } from 'dependency-path'
import path = require('path')
import normalizePath = require('normalize-path')
import realpathMissing = require('realpath-missing')
import resolveLinkTarget = require('resolve-link-target')

export type SearchFunction = (pkg: { name: string, version: string }) => boolean

export interface PackageNode {
  alias: string
  circular?: true
  dependencies?: PackageNode[]
  dev?: boolean
  isPeer: boolean
  isSkipped: boolean
  isMissing: boolean
  name: string
  optional?: true
  path: string
  resolved?: string
  searched?: true
  version: string
}

export interface DependenciesHierarchy {
  dependencies?: PackageNode[]
  devDependencies?: PackageNode[]
  optionalDependencies?: PackageNode[]
  unsavedDependencies?: PackageNode[]
}

export default async function dependenciesHierarchy (
  projectPaths: string[],
  maybeOpts: {
    depth: number
    include?: { [dependenciesField in DependenciesField]: boolean }
    registries?: Registries
    search?: SearchFunction
    lockfileDir: string
  }
): Promise<{ [projectDir: string]: DependenciesHierarchy }> {
  if (!maybeOpts || !maybeOpts.lockfileDir) {
    throw new TypeError('opts.lockfileDir is required')
  }
  const modulesDir = await realpathMissing(path.join(maybeOpts.lockfileDir, 'node_modules'))
  const modules = await readModulesYaml(modulesDir)
  const registries = normalizeRegistries({
    ...maybeOpts?.registries,
    ...modules?.registries,
  })
  const currentLockfile = (modules?.virtualStoreDir && await readCurrentLockfile(modules.virtualStoreDir, { ignoreIncompatible: false })) ?? null

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
    registries,
    search: maybeOpts.search,
    skipped: new Set(modules?.skipped ?? []),
  }
  ; (
    await Promise.all(projectPaths.map(async (projectPath) => {
      return [
        projectPath,
        await dependenciesHierarchyForPackage(projectPath, currentLockfile, opts),
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
  opts: {
    depth: number
    include: { [dependenciesField in DependenciesField]: boolean }
    registries: Registries
    search?: SearchFunction
    skipped: Set<string>
    lockfileDir: string
  }
) {
  const importerId = getLockfileImporterId(opts.lockfileDir, projectPath)

  if (!currentLockfile.importers[importerId]) return {}

  const modulesDir = path.join(projectPath, 'node_modules')

  const savedDeps = getAllDirectDependencies(currentLockfile.importers[importerId])
  const allDirectDeps = await readModulesDir(modulesDir) ?? []
  const unsavedDeps = allDirectDeps.filter((directDep) => !savedDeps[directDep])
  const wantedLockfile = await readWantedLockfile(opts.lockfileDir, { ignoreIncompatible: false }) ?? { packages: {} }

  const getChildrenTree = getTree.bind(null, {
    currentDepth: 1,
    currentPackages: currentLockfile.packages ?? {},
    includeOptionalDependencies: opts.include.optionalDependencies,
    lockfileDir: opts.lockfileDir,
    maxDepth: opts.depth,
    modulesDir,
    registries: opts.registries,
    search: opts.search,
    skipped: opts.skipped,
    wantedPackages: wantedLockfile.packages ?? {},
  })
  const result: DependenciesHierarchy = {}
  for (const dependenciesField of DEPENDENCIES_FIELDS.sort().filter(dependenciedField => opts.include[dependenciedField])) {
    const topDeps = currentLockfile.importers[importerId][dependenciesField] ?? {}
    result[dependenciesField] = []
    Object.keys(topDeps).forEach((alias) => {
      const { packageInfo, packageAbsolutePath } = getPkgInfo({
        alias,
        currentPackages: currentLockfile.packages ?? {},
        lockfileDir: opts.lockfileDir,
        modulesDir,
        ref: topDeps[alias],
        registries: opts.registries,
        skipped: opts.skipped,
        wantedPackages: wantedLockfile.packages ?? {},
      })
      let newEntry: PackageNode | null = null
      const matchedSearched = opts.search?.(packageInfo)
      if (packageAbsolutePath === null) {
        if (opts.search && !matchedSearched) return
        newEntry = packageInfo
      } else {
        const relativeId = refToRelative(topDeps[alias], alias)
        if (relativeId) {
          const dependencies = getChildrenTree([relativeId], relativeId)
          if (dependencies.length) {
            newEntry = {
              ...packageInfo,
              dependencies,
            }
          } else if (!opts.search || matchedSearched) {
            newEntry = packageInfo
          }
        }
      }
      if (newEntry) {
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
      } catch (err) {
        // if error happened. The package is not a link
        const pkg = await safeReadPackageFromDir(pkgPath)
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
      if (opts.search && !matchedSearched) return
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

function getAllDirectDependencies (projectSnapshot: ProjectSnapshot) {
  return {
    ...projectSnapshot.dependencies,
    ...projectSnapshot.devDependencies,
    ...projectSnapshot.optionalDependencies,
  }
}

interface GetTreeOpts {
  currentDepth: number
  maxDepth: number
  lockfileDir: string
  modulesDir: string
  includeOptionalDependencies: boolean
  search?: SearchFunction
  skipped: Set<string>
  registries: Registries
  currentPackages: PackageSnapshots
  wantedPackages: PackageSnapshots
}

interface DependencyInfo { circular?: true, dependencies: PackageNode[] }

function getTree (
  opts: GetTreeOpts,
  keypath: string[],
  parentId: string
): PackageNode[] {
  const dependenciesCache = new Map<string, PackageNode[]>()

  return getTreeHelper(dependenciesCache, opts, keypath, parentId).dependencies
}

function getTreeHelper (
  dependenciesCache: Map<string, PackageNode[]>,
  opts: GetTreeOpts,
  keypath: string[],
  parentId: string
): DependencyInfo {
  const result: DependencyInfo = { dependencies: [] }
  if (opts.currentDepth > opts.maxDepth || !opts.currentPackages || !opts.currentPackages[parentId]) return result

  const deps = !opts.includeOptionalDependencies
    ? opts.currentPackages[parentId].dependencies
    : {
      ...opts.currentPackages[parentId].dependencies,
      ...opts.currentPackages[parentId].optionalDependencies,
    }

  if (!deps) return result

  const getChildrenTree = getTreeHelper.bind(null, dependenciesCache, {
    ...opts,
    currentDepth: opts.currentDepth + 1,
  })

  const peers = new Set(Object.keys(opts.currentPackages[parentId].peerDependencies ?? {}))

  Object.keys(deps).forEach((alias) => {
    const { packageInfo, packageAbsolutePath } = getPkgInfo({
      alias,
      currentPackages: opts.currentPackages,
      lockfileDir: opts.lockfileDir,
      modulesDir: opts.modulesDir,
      peers,
      ref: deps[alias],
      registries: opts.registries,
      skipped: opts.skipped,
      wantedPackages: opts.wantedPackages,
    })
    let circular: boolean
    const matchedSearched = opts.search?.(packageInfo)
    let newEntry: PackageNode | null = null
    if (packageAbsolutePath === null) {
      circular = false
      newEntry = packageInfo
    } else {
      let dependencies: PackageNode[] | undefined

      const relativeId = refToRelative(deps[alias], alias) as string // we know for sure that relative is not null if pkgPath is not null
      circular = keypath.includes(relativeId)

      if (circular) {
        dependencies = []
      } else {
        dependencies = dependenciesCache.get(packageAbsolutePath)

        if (!dependencies) {
          const children = getChildrenTree(keypath.concat([relativeId]), relativeId)
          dependencies = children.dependencies

          if (children.circular) {
            result.circular = true
          } else {
            dependenciesCache.set(packageAbsolutePath, dependencies)
          }
        }
      }

      if (dependencies.length) {
        newEntry = {
          ...packageInfo,
          dependencies,
        }
      } else if (!opts.search || matchedSearched) {
        newEntry = packageInfo
      }
    }
    if (newEntry) {
      if (circular) {
        newEntry.circular = true
        result.circular = true
      }
      if (matchedSearched) {
        newEntry.searched = true
      }
      result.dependencies.push(newEntry)
    }
  })

  return result
}

function getPkgInfo (
  opts: {
    alias: string
    lockfileDir: string
    modulesDir: string
    ref: string
    currentPackages: PackageSnapshots
    peers?: Set<string>
    registries: Registries
    skipped: Set<string>
    wantedPackages: PackageSnapshots
  }
) {
  let name!: string
  let version!: string
  let resolved: string | undefined
  let dev: boolean | undefined
  let optional: true | undefined
  let isSkipped: boolean = false
  let isMissing: boolean = false
  const depPath = refToRelative(opts.ref, opts.alias)
  if (depPath) {
    let pkgSnapshot!: PackageSnapshot
    if (opts.currentPackages[depPath]) {
      pkgSnapshot = opts.currentPackages[depPath]
      const parsed = nameVerFromPkgSnapshot(depPath, pkgSnapshot)
      name = parsed.name
      version = parsed.version
    } else {
      pkgSnapshot = opts.wantedPackages[depPath]
      if (pkgSnapshot) {
        const parsed = nameVerFromPkgSnapshot(depPath, pkgSnapshot)
        name = parsed.name
        version = parsed.version
      } else {
        name = opts.alias
        version = opts.ref
      }
      isMissing = true
      isSkipped = opts.skipped.has(depPath)
    }
    resolved = pkgSnapshotToResolution(depPath, pkgSnapshot, opts.registries)['tarball']
    dev = pkgSnapshot.dev
    optional = pkgSnapshot.optional
  } else {
    name = opts.alias
    version = opts.ref
  }
  const packageAbsolutePath = refToRelative(opts.ref, opts.alias)
  const packageInfo = {
    alias: opts.alias,
    isMissing,
    isPeer: Boolean(opts.peers?.has(opts.alias)),
    isSkipped,
    name,
    path: depPath ? path.join(opts.modulesDir, '.pnpm', pkgIdToFilename(depPath, opts.lockfileDir)) : path.join(opts.modulesDir, '..', opts.ref.substr(5)),
    version,
  }
  if (resolved) {
    packageInfo['resolved'] = resolved
  }
  if (optional === true) {
    packageInfo['optional'] = true
  }
  if (typeof dev === 'boolean') {
    packageInfo['dev'] = dev
  }
  return {
    packageAbsolutePath,
    packageInfo,
  }
}
