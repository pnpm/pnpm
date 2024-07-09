import { type PackageNode } from '@pnpm/reviewing.dependencies-hierarchy'
import sortBy from 'ramda/src/sortBy'
import prop from 'ramda/src/prop'
import { type PackageDependencyHierarchy } from './types'

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
    .filter(p => p.length !== 0)
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
  const pkgs = sortPackages(
    flatten(
      depPaths,
      [
        ...(pkg.optionalDependencies ?? []),
        ...(pkg.dependencies ?? []),
        ...(pkg.devDependencies ?? []),
        ...(pkg.unsavedDependencies ?? []),
      ]
    )
  )
  if (!opts.alwaysPrintRootPackage && (pkgs.length === 0)) return ''
  if (opts.long) {
    let firstLine = pkg.path
    if (pkg.name) {
      firstLine += `:${pkg.name}`
      if (pkg.version) {
        firstLine += `@${pkg.version}`
      }
      if (pkg.private) {
        firstLine += ':PRIVATE'
      }
    }
    return [
      firstLine,
      ...pkgs.map((pkg) => `${pkg.path}:${pkg.name}@${pkg.version}`),
    ].join('\n')
  }
  return [
    pkg.path,
    ...pkgs.map((pkg) => pkg.path),
  ].join('\n')
}

interface PackageInfo {
  name: string
  version: string
  path: string
}

function flatten (
  depPaths: Set<string>,
  nodes: PackageNode[]
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
