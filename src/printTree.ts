import archy = require('archy')
import dh, {
  forPackages as dhForPackages,
  PackageNode,
  PackageSelector,
} from 'dependencies-hierarchy'
import path = require('path')
import chalk = require('chalk')
import readPkg from './readPkg'

export default async function (
  projectPath: string,
  tree: PackageNode[],
  opts: {
    long: boolean,
  }
) {
  const pkg = await readPkg('package.json')

  const s = archy({
    label: `${pkg.name}@${pkg.version} ${projectPath}`,
    nodes: await toArchyTree(tree, {
      long: opts.long,
      modules: path.join(projectPath, 'node_modules')
    }),
  })

  return s
}

async function toArchyTree (
  nodes: PackageNode[],
  opts: {
    long: boolean,
    modules: string,
  }
): Promise<archy.Data[]> {
  return Promise.all(
    nodes.map(async node => {
      const nodes = await toArchyTree(node.dependencies || [], opts)
      if (opts.long) {
        const pkg = await readPkg(path.join(opts.modules, `.${node.pkg.path}`, 'node_modules', node.pkg.name, 'package.json'))
        const labelLines = [
          printLabel(node),
          pkg.description
        ]
        if (pkg.repository) {
          labelLines.push(pkg.repository.url)
        }
        if (pkg.homepage) {
          labelLines.push(pkg.homepage)
        }
        return {
          label: labelLines.join('\n'),
          nodes,
        }
      }
      return {
        label: printLabel(node),
        nodes,
      }
    })
  )
}

function printLabel (node: PackageNode) {
  const txt = `${node.pkg.name}@${node.pkg.version}`
  if (node.searched) {
    return chalk.yellow.bgBlack(txt)
  }
  return txt
}
