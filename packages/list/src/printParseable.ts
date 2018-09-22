import {PackageNode} from 'dependencies-hierarchy'
import path = require('path')
import R = require('ramda')
import readPkg from './readPkg'

const sortPackages = R.sortBy(R.prop('name'))

export default async function(
  projectPath: string,
  tree: PackageNode[],
  opts: {
    long: boolean,
    alwaysPrintRootPackage: boolean,
  },
) {
  const pkgs = sortPackages(flatten(tree))
  const prefix = path.join(projectPath, 'node_modules')
  if (!opts.alwaysPrintRootPackage && !pkgs.length) return ''
  if (opts.long) {
    const entryPkg = await readPkg(path.resolve(projectPath, 'package.json'))
    let firstLine = projectPath
    if (entryPkg.name) {
      firstLine += `:${entryPkg.name}`
      if (entryPkg.version) {
        firstLine += `@${entryPkg.version}`
      }
    }
    return `${firstLine}\n` +
      pkgs.map((pkg) => `${prefix}/.${pkg.path}:${pkg.name}@${pkg.version}`).join('\n') + '\n'
  }
  return projectPath + '\n' + pkgs.map((pkg) => `${prefix}/.${pkg.path}`).join('\n') + '\n'
}

interface PackageInfo {name: string, version: string, path: string}

function flatten(
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
