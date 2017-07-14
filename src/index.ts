import {
  readPrivate,
  Shrinkwrap,
  ResolvedPackages,
  getPkgShortId,
} from 'pnpm-shrinkwrap'
import semver = require('semver')

export type SearchedPackage = {
  name: string,
  range: string,
}

export type PackageNode = {
  pkg: {
    name: string,
    version: string,
  }
  dependencies?: PackageNode[],
}

export default async function list (
  projectPath: string,
  opts?: {
    depth: number,
    only?: 'dev' | 'prod',
    searched?: SearchedPackage[],
  }
): Promise<PackageNode[]> {
  const _opts = Object.assign({}, {
    depth: 0,
    only: undefined,
    searched: [],
  }, opts)
  const shrinkwrap = await readPrivate(projectPath, {ignoreIncompatible: false})

  if (!shrinkwrap) return []

  const topDeps = getTopDependencies(shrinkwrap, _opts)

  if (!topDeps) return []

  const getChildrenTree = getTree.bind(null, {
    currentDepth: 1,
    maxDepth: _opts.depth,
    prod: _opts.only === 'prod',
    searched: _opts.searched,
  }, shrinkwrap.packages)
  const result: PackageNode[] = []
  Object.keys(topDeps).forEach(depName => {
    const shortId = getPkgShortId(topDeps[depName], depName)
    const pkg = {
      name: depName,
      version: topDeps[depName],
    }
    const dependencies = getChildrenTree(shortId)
    if (dependencies.length) {
      result.push({
        pkg,
        dependencies,
      })
      return
    }
    if (!_opts.searched.length || matches(_opts.searched, pkg)) {
      result.push({pkg})
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
  },
  packages: ResolvedPackages,
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
    const shortId = getPkgShortId(deps[depName], depName)
    const pkg = {
      name: depName,
      version: deps[depName],
    }
    const dependencies = getChildrenTree(shortId)
    if (dependencies.length) {
      result.push({
        pkg,
        dependencies,
      })
      return
    }
    if (!opts.searched.length || matches(opts.searched, pkg)) {
      result.push({pkg})
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
