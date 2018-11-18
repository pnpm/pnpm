import { PackageNode } from 'dependencies-hierarchy'
import path = require('path')
import R = require('ramda')
import readPkg from './readPkg'

const sortPackages = R.sortBy(R.prop('name'))

export default async function (
  project: {
    name: string,
    version: string,
    path: string,
  },
  tree: PackageNode[],
  opts: {
    long: boolean,
    alwaysPrintRootPackage: boolean,
  },
) {
  const pkgs = sortPackages(flatten(tree))
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
      '',
    ].join('\n')
  }
  return [
    project.path,
    ...pkgs.map((pkg) => pkg.path),
    '',
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
