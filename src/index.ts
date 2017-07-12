import {
  readPrivate,
  Shrinkwrap,
  ResolvedPackages,
  getPkgShortId,
} from 'pnpm-shrinkwrap'

async function list (
  projectPath: string,
  opts?: {
    depth: number,
    only?: 'dev' | 'prod',
  }
) {
  const _opts = Object.assign({}, {
    depth: 0,
    only: undefined,
  }, opts)
  const shrinkwrap = await readPrivate(projectPath, {ignoreIncompatible: false})

  if (!shrinkwrap) return {}

  const topDeps = getTopDependencies(shrinkwrap, _opts)

  if (!topDeps) return []

  const getChildrenTree = getTree.bind(null, {
    currentDepth: 1,
    maxDepth: _opts.depth,
    prod: _opts.only === 'prod',
  }, shrinkwrap.packages)
  return Object.keys(topDeps).map(depName => {
    const shortId = getPkgShortId(topDeps[depName], depName)
    const pkg = {
      name: depName,
      version: topDeps[depName],
    }
    const dependencies = getChildrenTree(shortId)
    if (dependencies.length) {
      return {
        pkg,
        dependencies,
      }
    }
    return {pkg}
  })
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
  },
  packages: ResolvedPackages,
  parentId: string
) {
  if (opts.currentDepth > opts.maxDepth) return []

  const deps = opts.prod
    ? packages[parentId].dependencies
    : Object.assign({},
      packages[parentId].dependencies,
      packages[parentId].optionalDependencies
    )

  if (!deps) return []

  const getChildrenTree = getTree.bind(null, {
    currentDepth: opts.currentDepth + 1,
    maxDepth: opts.maxDepth,
    prod: opts.prod,
  }, packages)
  return Object.keys(deps).map(depName => {
    const shortId = getPkgShortId(deps[depName], depName)
    const pkg = {
      name: depName,
      version: deps[depName],
    }
    const dependencies = getChildrenTree(shortId)
    if (dependencies.depth) {
      return {
        pkg,
        dependencies,
      }
    }
    return {pkg}
  })
}

export = list
