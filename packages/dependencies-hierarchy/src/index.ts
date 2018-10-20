import { read as readModulesYaml } from '@pnpm/modules-yaml'
import { Registries } from '@pnpm/types'
import { normalizeRegistries } from '@pnpm/utils'
import assert = require('assert')
import { refToAbsolute, refToRelative } from 'dependency-path'
import {
  getImporterId,
  readCurrent,
  ResolvedPackages,
  ShrinkwrapImporter,
} from 'pnpm-shrinkwrap'
import semver = require('semver')

export type PackageSelector = string | {
  name: string,
  range: string,
}

export interface PackageNode {
  pkg: {
    name: string,
    version: string,
    path: string,
  }
  dependencies?: PackageNode[],
  searched?: true,
  circular?: true,
}

export function forPackages (
  packages: PackageSelector[],
  projectPath: string,
  opts?: {
    depth: number,
    only?: 'dev' | 'prod',
    registries?: Registries,
    shrinkwrapDirectory?: string,
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
    shrinkwrapDirectory?: string,
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
    shrinkwrapDirectory?: string,
  },
): Promise<PackageNode[]> {
  const modules = await readModulesYaml(projectPath)
  const registries = normalizeRegistries({
    ...maybeOpts && maybeOpts.registries,
    ...modules && modules.registries,
  })
  const shrinkwrapDirectory = maybeOpts && maybeOpts.shrinkwrapDirectory || projectPath
  const shrinkwrap = await readCurrent(shrinkwrapDirectory, { ignoreIncompatible: false })

  if (!shrinkwrap) return []

  const opts = {
    depth: 0,
    only: undefined,
    ...maybeOpts,
  }
  const importerId = getImporterId(shrinkwrapDirectory, projectPath)
  const topDeps = getTopDependencies(shrinkwrap.importers[importerId], opts)

  if (!topDeps) return []

  const getChildrenTree = getTree.bind(null, {
    currentDepth: 1,
    maxDepth: opts.depth,
    prod: opts.only === 'prod',
    registry: shrinkwrap.registry,
    searched,
  }, shrinkwrap.packages)
  const result: PackageNode[] = []
  Object.keys(topDeps).forEach((depName) => {
    const pkgPath = refToAbsolute(topDeps[depName], depName, registries.default)
    const pkg = {
      name: depName,
      path: pkgPath || topDeps[depName],
      version: topDeps[depName],
    }
    let newEntry: PackageNode | null = null
    const matchedSearched = searched.length && matches(searched, pkg)
    if (pkgPath === null) {
      newEntry = { pkg }
    } else {
      const relativeId = refToRelative(topDeps[depName], depName)
      const dependencies = getChildrenTree([relativeId], relativeId)
      if (dependencies.length) {
        newEntry = {
          dependencies,
          pkg,
        }
      } else if (!searched.length || matches(searched, pkg)) {
        newEntry = { pkg }
      }
    }
    if (newEntry) {
      if (matchedSearched) {
        newEntry.searched = true
      }
      result.push(newEntry)
    }
  })
  return result
}

function getTopDependencies (
  shrinkwrapImporter: ShrinkwrapImporter,
  opts: {
    only?: 'dev' | 'prod',
  },
) {
  switch (opts.only) {
    case 'prod':
      return shrinkwrapImporter.dependencies
    case 'dev':
      return shrinkwrapImporter.devDependencies
    default:
      return {
        ...shrinkwrapImporter.dependencies,
        ...shrinkwrapImporter.devDependencies,
        ...shrinkwrapImporter.optionalDependencies,
      }
  }
}

function getTree (
  opts: {
    currentDepth: number,
    maxDepth: number,
    prod: boolean,
    searched: PackageSelector[],
    registry: string,
  },
  packages: ResolvedPackages,
  keypath: string[],
  parentId: string,
): PackageNode[] {
  if (opts.currentDepth > opts.maxDepth || !packages || !packages[parentId]) return []

  const deps = opts.prod
    ? packages[parentId].dependencies
    : Object.assign({},
      packages[parentId].dependencies,
      packages[parentId].optionalDependencies,
    )

  if (!deps) return []

  const getChildrenTree = getTree.bind(null, Object.assign({}, opts, {
    currentDepth: opts.currentDepth + 1,
  }), packages)

  const result: PackageNode[] = []
  Object.keys(deps).forEach((depName) => {
    const pkgPath = refToAbsolute(deps[depName], depName, opts.registry)
    const pkg = {
      name: depName,
      path: pkgPath || deps[depName],
      version: deps[depName],
    }
    let circular: boolean
    const matchedSearched = opts.searched.length && matches(opts.searched, pkg)
    let newEntry: PackageNode | null = null
    if (pkgPath === null) {
      circular = false
      newEntry = { pkg }
    } else {
      const relativeId = refToRelative(deps[depName], depName) as string // we know for sure that relative is not null if pkgPath is not null
      circular = keypath.indexOf(relativeId) !== -1
      const dependencies = circular ? [] : getChildrenTree(keypath.concat([relativeId]), relativeId)

      if (dependencies.length) {
        newEntry = {
          dependencies,
          pkg,
        }
      } else if (!opts.searched.length || matchedSearched) {
        newEntry = { pkg }
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

function matches (
  searched: PackageSelector[],
  pkg: {name: string, version: string},
) {
  return searched.some((searchedPkg) => {
    if (typeof searchedPkg === 'string') {
      return pkg.name === searchedPkg
    }
    return searchedPkg.name === pkg.name &&
      semver.satisfies(pkg.version, searchedPkg.range)
  })
}
