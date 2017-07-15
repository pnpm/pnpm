import {
  readPrivate,
  Shrinkwrap,
  ResolvedPackages,
} from 'pnpm-shrinkwrap'
import semver = require('semver')
import {refToAbsolute, refToRelative} from 'dependency-path'
import assert = require('assert')

export type SearchedPackage = {
  name: string,
  range: string,
}

export type PackageNode = {
  pkg: {
    name: string,
    version: string,
    resolvedId: string,
  }
  dependencies?: PackageNode[],
  searched?: true,
  circular?: true,
}

export function forPackages (
  packages: SearchedPackage[],
  projectPath: string,
  opts?: {
    depth: number,
    only?: 'dev' | 'prod',
  }
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
  }
) {
  return dependenciesHierarchy(projectPath, [], opts)
}

async function dependenciesHierarchy (
  projectPath: string,
  searched: SearchedPackage[],
  opts?: {
    depth: number,
    only?: 'dev' | 'prod',
  }
): Promise<PackageNode[]> {
  const _opts = Object.assign({}, {
    depth: 0,
    only: undefined,
  }, opts)
  const shrinkwrap = await readPrivate(projectPath, {ignoreIncompatible: false})

  if (!shrinkwrap) return []

  const topDeps = getTopDependencies(shrinkwrap, _opts)

  if (!topDeps) return []

  const getChildrenTree = getTree.bind(null, {
    currentDepth: 1,
    maxDepth: _opts.depth,
    prod: _opts.only === 'prod',
    searched,
    registry: shrinkwrap.registry,
  }, shrinkwrap.packages)
  const result: PackageNode[] = []
  Object.keys(topDeps).forEach(depName => {
    const relativeId = refToRelative(topDeps[depName], depName)
    const resolvedId = refToAbsolute(topDeps[depName], depName, shrinkwrap.registry)
    const pkg = {
      resolvedId,
      name: depName,
      version: topDeps[depName],
    }
    const dependencies = getChildrenTree([relativeId], relativeId)
    let newEntry: PackageNode | null = null
    const matchedSearched = searched.length && matches(searched, pkg)
    if (dependencies.length) {
      newEntry = {
        pkg,
        dependencies,
      }
    } else if (!searched.length || matches(searched, pkg)) {
      newEntry = {pkg}
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
  shrinkwrap: Shrinkwrap,
  opts: {
    only?: 'dev' | 'prod',
  }
) {
  switch (opts.only) {
    case 'prod':
      return shrinkwrap.dependencies
    case 'dev':
      return shrinkwrap.devDependencies
    default:
      return Object.assign({},
        shrinkwrap.dependencies,
        shrinkwrap.devDependencies,
        shrinkwrap.optionalDependencies
      )
  }
}

function getTree (
  opts: {
    currentDepth: number,
    maxDepth: number,
    prod: boolean,
    searched: SearchedPackage[],
    registry: string,
  },
  packages: ResolvedPackages,
  keypath: string[],
  parentId: string
): PackageNode[] {
  if (opts.currentDepth > opts.maxDepth) return []

  const deps = opts.prod
    ? packages[parentId].dependencies
    : Object.assign({},
      packages[parentId].dependencies,
      packages[parentId].optionalDependencies
    )

  if (!deps) return []

  const getChildrenTree = getTree.bind(null, Object.assign({}, opts, {
    currentDepth: opts.currentDepth + 1,
  }), packages)

  let result: PackageNode[] = []
  Object.keys(deps).forEach(depName => {
    const resolvedId = refToAbsolute(deps[depName], depName, opts.registry)
    const relativeId = refToRelative(deps[depName], depName)
    const pkg = {
      resolvedId,
      name: depName,
      version: deps[depName],
    }
    const circular = keypath.indexOf(relativeId) !== -1
    const dependencies = circular ? [] : getChildrenTree(keypath.concat([relativeId]), relativeId)
    let newEntry: PackageNode | null = null
    const matchedSearched = opts.searched.length && matches(opts.searched, pkg)
    if (dependencies.length) {
      newEntry = {
        pkg,
        dependencies,
      }
    } else if (!opts.searched.length || matchedSearched) {
      newEntry = {pkg}
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
  searched: SearchedPackage[],
  pkg: {name: string, version: string}
) {
  return searched.some(searchedPkg => searchedPkg.name === pkg.name && semver.satisfies(pkg.version, searchedPkg.range))
}
