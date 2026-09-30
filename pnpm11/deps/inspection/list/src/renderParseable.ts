import type { DependencyNode } from '@pnpm/deps.inspection.tree-builder'
import { prop, sortBy } from 'ramda'

import type { PackageDependencyHierarchy } from './types.js'

const sortPackages = sortBy(prop('name'))

export async function renderParseable (
  pkgs: PackageDependencyHierarchy[],
  opts: {
    long: boolean
    depth: number
    alwaysPrintRootPackage: boolean
    search: boolean
  }
): Promise<string> {
  const depPaths = new Set<string>()
  return pkgs
    .map(renderParseableForPackage.bind(null, depPaths, opts))
    .filter((rendered) => rendered.length !== 0)
    .join('\n')
}

function renderParseableForPackage (
  depPaths: Set<string>,
  opts: {
    long: boolean
    depth: number
    alwaysPrintRootPackage: boolean
    search: boolean
  },
  pkg: PackageDependencyHierarchy
): string {
  const rootAlreadySeen = depPaths.has(pkg.path)
  depPaths.add(pkg.path)
  const allDeps = [
    ...(pkg.optionalDependencies ?? []),
    ...(pkg.dependencies ?? []),
    ...(pkg.devDependencies ?? []),
    ...(pkg.unsavedDependencies ?? []),
  ]
  const pkgs = sortPackages(flatten(depPaths, allDeps))
  if (rootAlreadySeen && pkgs.length === 0) return ''
  if (!opts.alwaysPrintRootPackage && pkgs.length === 0 && allDeps.length === 0) return ''
  if (opts.long) {
    return [
      ...(rootAlreadySeen ? [] : [renderLongRootLine(pkg)]),
      ...pkgs.map((pkgNode) => renderLongDependencyLine(pkgNode as DependencyNode)),
    ].join('\n')
  }
  return [
    ...(rootAlreadySeen ? [] : [pkg.path]),
    ...pkgs.map((pkg) => pkg.path),
  ].join('\n')
}

function renderLongRootLine (pkg: PackageDependencyHierarchy): string {
  if (!pkg.name) return pkg.path
  let firstLine = `${pkg.path}:${pkg.name}`
  if (pkg.version) {
    firstLine += `@${pkg.version}`
  }
  if (pkg.private) {
    firstLine += ':PRIVATE'
  }
  return firstLine
}

function renderLongDependencyLine (node: DependencyNode): string {
  if (node.alias !== node.name) {
    // Only add npm: prefix if version doesn't already contain @ (to avoid file:, link:, etc.)
    if (!node.version.includes('@')) {
      return `${node.path}:${node.alias} npm:${node.name}@${node.version}`
    }
    return `${node.path}:${node.alias} ${node.version}`
  }
  // If version already contains @, it's in full format (e.g., name@file:path)
  if (node.version.includes('@')) {
    return `${node.path}:${node.version}`
  }
  return `${node.path}:${node.name}@${node.version}`
}

interface PackageInfo {
  alias: string
  name: string
  version: string
  path: string
}

function flatten (
  depPaths: Set<string>,
  nodes: DependencyNode[]
): PackageInfo[] {
  let packages: PackageInfo[] = []
  for (const node of nodes) {
    // The content output by renderParseable is flat,
    // so we can deduplicate packages that are repeatedly dependent on multiple packages.
    if (!depPaths.has(node.path)) {
      depPaths.add(node.path)
      packages.push(node)
    }
    if (node.dependencies?.length) {
      packages = packages.concat(flatten(depPaths, node.dependencies))
    }
  }
  return packages
}
