import {
  getLockfileImporterId,
  Lockfile,
  LockfileImporter,
  PackageSnapshot,
  PackageSnapshots,
  readCurrentLockfile,
  readWantedLockfile,
} from '@pnpm/lockfile-file'
import {
  nameVerFromPkgSnapshot,
  pkgSnapshotToResolution,
} from '@pnpm/lockfile-utils'
import { read as readModulesYaml } from '@pnpm/modules-yaml'
import readModulesDir from '@pnpm/read-modules-dir'
import { DEPENDENCIES_FIELDS, DependenciesField, Registries } from '@pnpm/types'
import {
  normalizeRegistries,
  realNodeModulesDir,
  safeReadPackageFromDir,
} from '@pnpm/utils'
import assert = require('assert')
import { refToAbsolute, refToRelative } from 'dependency-path'
import { isMatch } from 'micromatch'
import normalizePath = require('normalize-path')
import path = require('path')
import resolveLinkTarget = require('resolve-link-target')
import semver = require('semver')

export type PackageSelector = string | {
  name: string,
  range: string,
}

export interface PackageNode {
  alias: string,
  circular?: true,
  dependencies?: PackageNode[],
  dev?: boolean,
  isPeer: boolean,
  isSkipped: boolean,
  isMissing: boolean,
  name: string,
  optional?: true,
  path: string,
  resolved?: string,
  searched?: true,
  version: string,
}

export function forPackages (
  packages: PackageSelector[],
  projectPaths: string[],
  opts: {
    depth: number,
    include?: { [dependenciesField in DependenciesField]: boolean },
    registries?: Registries,
    lockfileDirectory: string,
  },
) {
  assert(packages, 'packages should be defined')
  if (!packages.length) return {}

  return dependenciesHierarchy(projectPaths, packages, opts)
}

export default function (
  projectPaths: string[],
  opts: {
    depth: number,
    include?: { [dependenciesField in DependenciesField]: boolean },
    registries?: Registries,
    lockfileDirectory: string,
  },
) {
  return dependenciesHierarchy(projectPaths, [], opts)
}

export type DependenciesHierarchy = {
  dependencies?: PackageNode[],
  devDependencies?: PackageNode[],
  optionalDependencies?: PackageNode[],
  unsavedDependencies?: PackageNode[],
}

async function dependenciesHierarchy (
  projectPaths: string[],
  searched: PackageSelector[],
  maybeOpts: {
    depth: number,
    include?: { [dependenciesField in DependenciesField]: boolean },
    registries?: Registries,
    lockfileDirectory: string,
  },
): Promise<{ [prefix: string]: DependenciesHierarchy }> {
  if (!maybeOpts || !maybeOpts.lockfileDirectory) {
    throw new TypeError('opts.lockfileDirectory is required')
  }
  const virtualStoreDir = await realNodeModulesDir(maybeOpts.lockfileDirectory)
  const modules = await readModulesYaml(virtualStoreDir)
  const registries = normalizeRegistries({
    ...maybeOpts && maybeOpts.registries,
    ...modules && modules.registries,
  })
  const currentLockfile = await readCurrentLockfile(maybeOpts.lockfileDirectory, { ignoreIncompatible: false })

  const result = {} as { [prefix: string]: DependenciesHierarchy }

  if (!currentLockfile) {
    for (let projectPath of projectPaths) {
      result[projectPath] = {}
    }
    return result
  }

  const opts = {
    depth: maybeOpts.depth || 0,
    include: maybeOpts.include || {
      dependencies: true,
      devDependencies: true,
      optionalDependencies: true,
    },
    lockfileDirectory: maybeOpts.lockfileDirectory,
    registries,
    skipped: new Set(modules && modules.skipped || []),
  }
  ; (
    await Promise.all(projectPaths.map(async (projectPath) => {
      return [
        projectPath,
        await dependenciesHierarchyForPackage(projectPath, currentLockfile, searched, opts),
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
  searched: PackageSelector[],
  opts: {
    depth: number,
    include: { [dependenciesField in DependenciesField]: boolean },
    registries: Registries,
    skipped: Set<string>,
    lockfileDirectory: string,
  },
) {
  const importerId = getLockfileImporterId(opts.lockfileDirectory, projectPath)

  if (!currentLockfile.importers[importerId]) return {}

  const modulesDir = path.join(projectPath, 'node_modules')

  const savedDeps = getAllDirectDependencies(currentLockfile.importers[importerId])
  const allDirectDeps = await readModulesDir(modulesDir) || []
  const unsavedDeps = allDirectDeps.filter((directDep) => !savedDeps[directDep])
  const wantedLockfile = await readWantedLockfile(opts.lockfileDirectory, { ignoreIncompatible: false }) || { packages: {} }

  const getChildrenTree = getTree.bind(null, {
    currentDepth: 1,
    currentPackages: currentLockfile.packages || {},
    includeOptionalDependencies: opts.include.optionalDependencies === true,
    maxDepth: opts.depth,
    modulesDir,
    registries: opts.registries,
    searched,
    skipped: opts.skipped,
    wantedPackages: wantedLockfile.packages || {},
  })
  const result: DependenciesHierarchy = {}
  for (const dependenciesField of DEPENDENCIES_FIELDS.sort().filter(dependenciedField => opts.include[dependenciedField])) {
    const topDeps = currentLockfile.importers[importerId][dependenciesField] || {}
    result[dependenciesField] = []
    Object.keys(topDeps).forEach((alias) => {
      const { packageInfo, packageAbsolutePath } = getPkgInfo({
        alias,
        currentPackages: currentLockfile.packages || {},
        modulesDir,
        ref: topDeps[alias],
        registries: opts.registries,
        skipped: opts.skipped,
        wantedPackages: wantedLockfile.packages || {},
      })
      let newEntry: PackageNode | null = null
      const matchedSearched = searched.length && matches(searched, packageInfo)
      if (packageAbsolutePath === null) {
        if (searched.length && !matchedSearched) return
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
          } else if (!searched.length || matches(searched, packageInfo)) {
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
        version = pkg && pkg.version || 'undefined'
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
      const matchedSearched = searched.length && matches(searched, pkg)
      if (searched.length && !matchedSearched) return
      const newEntry: PackageNode = pkg
      if (matchedSearched) {
        newEntry.searched = true
      }
      result.unsavedDependencies = result.unsavedDependencies || []
      result.unsavedDependencies.push(newEntry)
    })
  )

  return result
}

function getAllDirectDependencies (lockfileImporter: LockfileImporter) {
  return {
    ...lockfileImporter.dependencies,
    ...lockfileImporter.devDependencies,
    ...lockfileImporter.optionalDependencies,
  }
}

function getTree (
  opts: {
    currentDepth: number,
    maxDepth: number,
    modulesDir: string,
    includeOptionalDependencies: boolean,
    searched: PackageSelector[],
    skipped: Set<string>,
    registries: Registries,
    currentPackages: PackageSnapshots,
    wantedPackages: PackageSnapshots,
  },
  keypath: string[],
  parentId: string,
): PackageNode[] {
  if (opts.currentDepth > opts.maxDepth || !opts.currentPackages || !opts.currentPackages[parentId]) return []

  const deps = opts.includeOptionalDependencies === false
    ? opts.currentPackages[parentId].dependencies
    : {
      ...opts.currentPackages[parentId].dependencies,
      ...opts.currentPackages[parentId].optionalDependencies,
    }

  if (!deps) return []

  const getChildrenTree = getTree.bind(null, {
    ...opts,
    currentDepth: opts.currentDepth + 1,
  })

  const peers = new Set(Object.keys(opts.currentPackages[parentId].peerDependencies || {}))
  const result: PackageNode[] = []
  Object.keys(deps).forEach((alias) => {
    const { packageInfo, packageAbsolutePath } = getPkgInfo({
      alias,
      currentPackages: opts.currentPackages,
      modulesDir: opts.modulesDir,
      peers,
      ref: deps[alias],
      registries: opts.registries,
      skipped: opts.skipped,
      wantedPackages: opts.wantedPackages,
    })
    let circular: boolean
    const matchedSearched = opts.searched.length && matches(opts.searched, packageInfo)
    let newEntry: PackageNode | null = null
    if (packageAbsolutePath === null) {
      circular = false
      newEntry = packageInfo
    } else {
      const relativeId = refToRelative(deps[alias], alias) as string // we know for sure that relative is not null if pkgPath is not null
      circular = keypath.includes(relativeId)
      const dependencies = circular ? [] : getChildrenTree(keypath.concat([relativeId]), relativeId)

      if (dependencies.length) {
        newEntry = {
          ...packageInfo,
          dependencies,
        }
      } else if (!opts.searched.length || matchedSearched) {
        newEntry = packageInfo
      }
    }
    if (newEntry) {
      if (circular) {
        newEntry.circular = true
      }
      if (matchedSearched) {
        newEntry.searched = true
      }
      result.push(newEntry)
    }
  })
  return result
}

function getPkgInfo (
  opts: {
    alias: string,
    modulesDir: string,
    ref: string,
    currentPackages: PackageSnapshots,
    peers?: Set<string>,
    registries: Registries,
    skipped: Set<string>,
    wantedPackages: PackageSnapshots,
  },
) {
  let name!: string
  let version!: string
  let resolved: string | undefined = undefined
  let dev: boolean | undefined = undefined
  let optional: true | undefined = undefined
  let isSkipped: boolean = false
  let isMissing: boolean = false
  const relDepPath = refToRelative(opts.ref, opts.alias)
  if (relDepPath) {
    let pkgSnapshot!: PackageSnapshot
    if (opts.currentPackages[relDepPath]) {
      pkgSnapshot = opts.currentPackages[relDepPath]
      const parsed = nameVerFromPkgSnapshot(relDepPath, pkgSnapshot)
      name = parsed.name
      version = parsed.version
    } else {
      pkgSnapshot = opts.wantedPackages[relDepPath]
      const parsed = nameVerFromPkgSnapshot(relDepPath, pkgSnapshot)
      name = parsed.name
      version = parsed.version
      isMissing = true
      isSkipped = opts.skipped.has(relDepPath)
    }
    resolved = pkgSnapshotToResolution(relDepPath, pkgSnapshot, opts.registries)['tarball']
    dev = pkgSnapshot.dev
    optional = pkgSnapshot.optional
  } else {
    name = opts.alias
    version = opts.ref
  }
  const packageAbsolutePath = refToAbsolute(opts.ref, opts.alias, opts.registries)
  const packageInfo = {
    alias: opts.alias,
    isMissing,
    isPeer: Boolean(opts.peers && opts.peers.has(opts.alias)),
    isSkipped,
    name,
    path: packageAbsolutePath && path.join(opts.modulesDir, '.pnpm', packageAbsolutePath) || path.join(opts.modulesDir, '..', opts.ref.substr(5)),
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

function matches (
  searched: PackageSelector[],
  pkg: {name: string, version: string},
) {
  return searched.some((searchedPkg) => {
    if (typeof searchedPkg === 'string') {
      return isMatch(pkg.name, searchedPkg)
    }
    return isMatch(pkg.name, searchedPkg.name) &&
      !pkg.version.startsWith('link:') &&
      semver.satisfies(pkg.version, searchedPkg.range)
  })
}
