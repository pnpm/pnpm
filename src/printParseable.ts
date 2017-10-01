import path = require('path')
import {PackageNode} from 'dependencies-hierarchy'
import readPkg from './readPkg'
import R = require('ramda')

const sortPackages = R.sortBy(R.prop('name'))

export default async function (
  projectPath: string,
  tree: PackageNode[],
  opts: {
    long: boolean,
  }
) {
  const pkgs = sortPackages(flatten(tree))
  const prefix = path.join(projectPath, 'node_modules')
  if (opts.long) {
    const pkg = await readPkg(path.resolve(projectPath, 'package.json'))
    return `${projectPath}:${pkg.name}@${pkg.version}\n` +
      pkgs.map(pkg => `${prefix}/.${pkg.path}:${pkg.name}@${pkg.version}`).join('\n') + '\n'
  }
  return projectPath + '\n' + pkgs.map(pkg => `${prefix}/.${pkg.path}`).join('\n') + '\n'
}

type PackageInfo = {name: string, version: string, path: string}

function flatten (
  nodes: PackageNode[]
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
