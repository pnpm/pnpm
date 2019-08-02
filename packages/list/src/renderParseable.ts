import { DependenciesHierarchy, PackageNode } from 'dependencies-hierarchy'
import R = require('ramda')

const sortPackages = R.sortBy(R.prop('name'))

export default async function (
  project: {
    name?: string,
    version?: string,
    path: string,
  },
  tree: DependenciesHierarchy,
  opts: {
    long: boolean,
    alwaysPrintRootPackage: boolean,
  },
) {
  const pkgs = sortPackages(
    flatten(
      [
        ...(tree.optionalDependencies || []),
        ...(tree.dependencies || []),
        ...(tree.devDependencies || []),
      ],
    ),
  )
  if (!opts.alwaysPrintRootPackage && !pkgs.length) return ''
  if (opts.long) {
    let firstLine = project.path
    if (project.name) {
      firstLine += `:${project.name}`
      if (project.version) {
        firstLine += `@${project.version}`
      }
    }
    return [
      firstLine,
      ...pkgs.map((pkg) => `${pkg.path}:${pkg.name}@${pkg.version}`),
    ].join('\n')
  }
  return [
    project.path,
    ...pkgs.map((pkg) => pkg.path),
  ].join('\n')
}

interface PackageInfo {name: string, version: string, path: string}

function flatten (
  nodes: PackageNode[],
): PackageInfo[] {
  let packages: PackageInfo[] = []
  for (const node of nodes) {
    packages.push(node.pkg)
    if (node.dependencies && node.dependencies.length) {
      packages = packages.concat(flatten(node.dependencies))
    }
  }
  return packages
}
