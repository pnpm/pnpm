import {
  getLockfileImporterId,
  LockfileImporter,
  PackageSnapshots,
  readCurrentLockfile,
} from '@pnpm/lockfile-file'
import {
  nameVerFromPkgSnapshot,
  pkgSnapshotToResolution,
} from '@pnpm/lockfile-utils'
import { read as readModulesYaml } from '@pnpm/modules-yaml'
import readModulesDir from '@pnpm/read-modules-dir'
import { DEPENDENCIES_FIELDS, DependenciesField, Registries } from '@pnpm/types'
import { normalizeRegistries, safeReadPackageFromDir } from '@pnpm/utils'
import assert = require('assert')
import { refToAbsolute, refToRelative } from 'dependency-path'
import normalizePath = require('normalize-path')
import path = require('path')
import resolveLinkTarget = require('resolve-link-target')
import semver = require('semver')

export type PackageSelector = string | {
  name: string,
  range: string,
}

export interface PackageNode {
  pkg: {
    alias: string,
    dev?: boolean,
    name: string,
    optional?: true,
    version: string,
    path: string,
    resolved?: string,
    isPeer: boolean,
  }
  dependencies?: PackageNode[],
  searched?: true,
  circular?: true,
  saved?: false,
}

export function forPackages (
  packages: PackageSelector[],
  projectPath: string,
  opts?: {
    depth: number,
    include?: { [dependenciesField in DependenciesField]: boolean },
    registries?: Registries,
    lockfileDirectory?: string,
  },
) {
  assert(packages, 'packages should be defined')
  if (!packages.length) return {}

  return dependenciesHierarchy(projectPath, packages, opts)
}

export default function (
  projectPath: string,
  opts?: {
    depth: number,
    include?: { [dependenciesField in DependenciesField]: boolean },
    registries?: Registries,
    lockfileDirectory?: string,
  },
) {
  return dependenciesHierarchy(projectPath, [], opts)
}

export type DependenciesHierarchy = {
  dependencies?: PackageNode[],
  devDependencies?: PackageNode[],
  optionalDependencies?: PackageNode[],
  unsavedDependencies?: PackageNode[],
}

async function dependenciesHierarchy (
  projectPath: string,
  searched: PackageSelector[],
  maybeOpts?: {
    depth: number,
    include?: { [dependenciesField in DependenciesField]: boolean },
    registries?: Registries,
    lockfileDirectory?: string,
  },
): Promise<DependenciesHierarchy> {
  const modules = await readModulesYaml(projectPath)
  const registries = normalizeRegistries({
    ...maybeOpts && maybeOpts.registries,
    ...modules && modules.registries,
  })
  const lockfileDirectory = maybeOpts && maybeOpts.lockfileDirectory || projectPath
  const lockfile = await readCurrentLockfile(lockfileDirectory, { ignoreIncompatible: false })

  if (!lockfile) return {}

  const opts = {
    depth: 0,
    ...maybeOpts,
  }
  const include = maybeOpts && maybeOpts.include || {
    dependencies: true,
    devDependencies: true,
    optionalDependencies: true,
  }
  const importerId = getLockfileImporterId(lockfileDirectory, projectPath)

  if (!lockfile.importers[importerId]) return {}

  const modulesDir = path.join(projectPath, 'node_modules')

  const savedDeps = getAllDirectDependencies(lockfile.importers[importerId])
  const allDirectDeps = await readModulesDir(modulesDir) || []
  const unsavedDeps = allDirectDeps.filter((directDep) => !savedDeps[directDep])

  const getChildrenTree = getTree.bind(null, {
    currentDepth: 1,
    includeOptionalDependencies: include.optionalDependencies === true,
    maxDepth: opts.depth,
    modulesDir,
    registries,
    searched,
  }, lockfile.packages)
  const result: DependenciesHierarchy = {}
  for (const dependenciesField of DEPENDENCIES_FIELDS.sort().filter(dependenciedField => include[dependenciedField])) {
    const topDeps = lockfile.importers[importerId][dependenciesField] || {}
    result[dependenciesField] = []
    Object.keys(topDeps).forEach((alias) => {
      const { packageInfo, packageAbsolutePath } = getPkgInfo({
        alias,
        modulesDir,
        packages: lockfile.packages || {},
        ref: topDeps[alias],
        registries,
      })
      let newEntry: PackageNode | null = null
      const matchedSearched = searched.length && matches(searched, packageInfo)
      if (packageAbsolutePath === null) {
        if (searched.length && !matchedSearched) return
        newEntry = { pkg: packageInfo }
      } else {
        const relativeId = refToRelative(topDeps[alias], alias)
        const dependencies = getChildrenTree([relativeId], relativeId)
        if (dependencies.length) {
          newEntry = {
            dependencies,
            pkg: packageInfo,
          }
        } else if (!searched.length || matches(searched, packageInfo)) {
          newEntry = { pkg: packageInfo }
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
        isPeer: false,
        name: unsavedDep,
        path: pkgPath,
        version,
      }
      const matchedSearched = searched.length && matches(searched, pkg)
      if (searched.length && !matchedSearched) return
      const newEntry: PackageNode = {
        pkg,
        saved: false,
      }
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
    registries: Registries,
  },
  packages: PackageSnapshots,
  keypath: string[],
  parentId: string,
): PackageNode[] {
  if (opts.currentDepth > opts.maxDepth || !packages || !packages[parentId]) return []

  const deps = opts.includeOptionalDependencies === false
    ? packages[parentId].dependencies
    : {
      ...packages[parentId].dependencies,
      ...packages[parentId].optionalDependencies,
    }

  if (!deps) return []

  const getChildrenTree = getTree.bind(null, {
    ...opts,
    currentDepth: opts.currentDepth + 1,
  }, packages)

  const peers = new Set(Object.keys(packages[parentId].peerDependencies || {}))
  const result: PackageNode[] = []
  Object.keys(deps).forEach((alias) => {
    const { packageInfo, packageAbsolutePath } = getPkgInfo({
      alias,
      modulesDir: opts.modulesDir,
      packages,
      peers,
      ref: deps[alias],
      registries: opts.registries,
    })
    let circular: boolean
    const matchedSearched = opts.searched.length && matches(opts.searched, packageInfo)
    let newEntry: PackageNode | null = null
    if (packageAbsolutePath === null) {
      circular = false
      newEntry = { pkg: packageInfo }
    } else {
      const relativeId = refToRelative(deps[alias], alias) as string // we know for sure that relative is not null if pkgPath is not null
      circular = keypath.includes(relativeId)
      const dependencies = circular ? [] : getChildrenTree(keypath.concat([relativeId]), relativeId)

      if (dependencies.length) {
        newEntry = {
          dependencies,
          pkg: packageInfo,
        }
      } else if (!opts.searched.length || matchedSearched) {
        newEntry = { pkg: packageInfo }
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
    packages: PackageSnapshots,
    peers?: Set<string>,
    registries: Registries,
  },
) {
  let name!: string
  let version!: string
  let resolved: string | undefined = undefined
  let dev: boolean | undefined = undefined
  let optional: true | undefined = undefined
  const relDepPath = refToRelative(opts.ref, opts.alias)
  if (relDepPath) {
    const parsed = nameVerFromPkgSnapshot(relDepPath, opts.packages[relDepPath])
    name = parsed.name
    version = parsed.version
    resolved = pkgSnapshotToResolution(relDepPath, opts.packages[relDepPath], opts.registries)['tarball']
    dev = opts.packages[relDepPath].dev
    optional = opts.packages[relDepPath].optional
  } else {
    name = opts.alias
    version = opts.ref
  }
  const packageAbsolutePath = refToAbsolute(opts.ref, opts.alias, opts.registries)
  const packageInfo = {
    alias: opts.alias,
    isPeer: Boolean(opts.peers && opts.peers.has(opts.alias)),
    name,
    path: packageAbsolutePath && path.join(opts.modulesDir, `.${packageAbsolutePath}`) || path.join(opts.modulesDir, '..', opts.ref.substr(5)),
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
      return pkg.name === searchedPkg
    }
    return searchedPkg.name === pkg.name &&
      !pkg.version.startsWith('link:') &&
      semver.satisfies(pkg.version, searchedPkg.range)
  })
}
