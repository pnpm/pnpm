import {
  getLockfileImporterId,
  LockfileImporter,
  PackageSnapshots,
  readCurrentLockfile,
} from '@pnpm/lockfile-file'
import { nameVerFromPkgSnapshot } from '@pnpm/lockfile-utils'
import { read as readModulesYaml } from '@pnpm/modules-yaml'
import readModulesDir from '@pnpm/read-modules-dir'
import { Registries } from '@pnpm/types'
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
    name: string,
    version: string,
    path: string,
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
    only?: 'dev' | 'prod',
    registries?: Registries,
    lockfileDirectory?: string,
  },
) {
  assert(packages, 'packages should be defined')
  if (!packages.length) return []

  return dependenciesHierarchy(projectPath, packages, opts)
}

export default function (
  projectPath: string,
  opts?: {
    depth: number,
    only?: 'dev' | 'prod',
    registries?: Registries,
    lockfileDirectory?: string,
  },
) {
  return dependenciesHierarchy(projectPath, [], opts)
}

async function dependenciesHierarchy (
  projectPath: string,
  searched: PackageSelector[],
  maybeOpts?: {
    depth: number,
    only?: 'dev' | 'prod',
    registries?: Registries,
    lockfileDirectory?: string,
  },
): Promise<PackageNode[]> {
  const modules = await readModulesYaml(projectPath)
  const registries = normalizeRegistries({
    ...maybeOpts && maybeOpts.registries,
    ...modules && modules.registries,
  })
  const lockfileDirectory = maybeOpts && maybeOpts.lockfileDirectory || projectPath
  const lockfile = await readCurrentLockfile(lockfileDirectory, { ignoreIncompatible: false })

  if (!lockfile) return []

  const opts = {
    depth: 0,
    only: undefined,
    ...maybeOpts,
  }
  const importerId = getLockfileImporterId(lockfileDirectory, projectPath)

  if (!lockfile.importers[importerId]) return []

  const topDeps = getFilteredDependencies(lockfile.importers[importerId], opts) || {}
  const modulesDir = path.join(projectPath, 'node_modules')

  const savedDeps = getAllDirectDependencies(lockfile.importers[importerId])
  const allDirectDeps = await readModulesDir(modulesDir) || []
  const unsavedDeps = allDirectDeps.filter((directDep) => !savedDeps[directDep])

  if (Object.keys(topDeps).length === 0 && unsavedDeps.length === 0) return []

  const getChildrenTree = getTree.bind(null, {
    currentDepth: 1,
    maxDepth: opts.depth,
    modulesDir,
    prod: opts.only === 'prod',
    registries,
    searched,
  }, lockfile.packages)
  const result: PackageNode[] = []
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
      result.push(newEntry)
    }
  })

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
      result.push(newEntry)
    })
  )

  return result
}

function getFilteredDependencies (
  lockfileImporter: LockfileImporter,
  opts: {
    only?: 'dev' | 'prod',
  },
) {
  switch (opts.only) {
    case 'prod':
      return lockfileImporter.dependencies
    case 'dev':
      return lockfileImporter.devDependencies
    default:
      return getAllDirectDependencies(lockfileImporter)
  }
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
    prod: boolean,
    searched: PackageSelector[],
    registries: Registries,
  },
  packages: PackageSnapshots,
  keypath: string[],
  parentId: string,
): PackageNode[] {
  if (opts.currentDepth > opts.maxDepth || !packages || !packages[parentId]) return []

  const deps = opts.prod
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

  const result: PackageNode[] = []
  Object.keys(deps).forEach((alias) => {
    const { packageInfo, packageAbsolutePath } = getPkgInfo({
      alias,
      modulesDir: opts.modulesDir,
      packages,
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
    registries: Registries,
  },
) {
  let name!: string
  let version!: string
  const relDepPath = refToRelative(opts.ref, opts.alias)
  if (relDepPath) {
    const parsed = nameVerFromPkgSnapshot(relDepPath, opts.packages[relDepPath])
    name = parsed.name
    version = parsed.version
  } else {
    name = opts.alias
    version = opts.ref
  }
  const packageAbsolutePath = refToAbsolute(opts.ref, opts.alias, opts.registries)
  return {
    packageAbsolutePath,
    packageInfo: {
      alias: opts.alias,
      name,
      path: packageAbsolutePath && path.join(opts.modulesDir, `.${packageAbsolutePath}`) || path.join(opts.modulesDir, '..', opts.ref.substr(5)),
      version,
    },
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
