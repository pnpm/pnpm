import { PackageNode } from 'dependencies-hierarchy'
import sortBy from 'ramda/src/sortBy'
import prop from 'ramda/src/prop'
import { PackageDependencyHierarchy } from './types'

const sortPackages = sortBy(prop('name'))

export async function renderParseable (
  pkgs: PackageDependencyHierarchy[],
  opts: {
    long: boolean
    depth: number
    alwaysPrintRootPackage: boolean
    search: boolean
  }
) {
  return pkgs.map((pkg) => renderParseableForPackage(pkg, opts)).filter(p => p.length !== 0).join('\n')
}

function renderParseableForPackage (
  pkg: PackageDependencyHierarchy,
  opts: {
    long: boolean
    depth: number
    alwaysPrintRootPackage: boolean
    search: boolean
  }
) {
  const pkgs = sortPackages(
    flatten(
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

interface PackageInfo {name: string, version: string, path: string}

function flatten (
  nodes: PackageNode[]
): PackageInfo[] {
  let packages: PackageInfo[] = []
  for (const node of nodes) {
    packages.push(node)
    if (node.dependencies?.length) {
      packages = packages.concat(flatten(node.dependencies))
    }
  }
  return packages
}
