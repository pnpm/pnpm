import { PackageNode } from 'dependencies-hierarchy'
import { PackageDependencyHierarchy } from './types'
import R = require('ramda')

const sortPackages = R.sortBy(R.prop('name'))

export default async function (
  pkgs: PackageDependencyHierarchy[],
  opts: {
    long: boolean
    depth: number
    alwaysPrintRootPackage: boolean
    search: boolean
  }
) {
  return pkgs.map((pkg) => renderParseableForPackage(pkg, opts)).join('\n')
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
  if (!opts.alwaysPrintRootPackage && !pkgs.length) return ''
  if (opts.long) {
    let firstLine = pkg.path
    if (pkg.name) {
      firstLine += `:${pkg.name}`
      if (pkg.version) {
        firstLine += `@${pkg.version}`
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
