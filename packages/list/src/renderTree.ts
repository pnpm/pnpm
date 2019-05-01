import archy = require('archy')
import chalk from 'chalk'
import { PackageNode } from 'dependencies-hierarchy'
import path = require('path')
import R = require('ramda')
import readPkg from './readPkg'

const sortPackages = R.sortBy(R.path(['pkg', 'name']) as (pkg: object) => R.Ord)

export default async function (
  project: {
    name?: string,
    version?: string,
    path: string,
  },
  tree: PackageNode[],
  opts: {
    alwaysPrintRootPackage: boolean,
    long: boolean,
  },
) {
  if (!opts.alwaysPrintRootPackage && !tree.length) return ''

  let label = ''
  if (project.name) {
    label += project.name
    if (project.version) {
      label += `@${project.version}`
    }
    label += ' '
  }
  label += project.path
  const s = archy({
    label,
    nodes: await toArchyTree(tree, {
      long: opts.long,
      modules: path.join(project.path, 'node_modules'),
    }),
  })

  return s.replace(/\n$/, '')
}

async function toArchyTree (
  entryNodes: PackageNode[],
  opts: {
    long: boolean,
    modules: string,
  },
): Promise<archy.Data[]> {
  return Promise.all(
    sortPackages(entryNodes).map(async (node) => {
      const nodes = await toArchyTree(node.dependencies || [], opts)
      if (opts.long) {
        const pkg = await readPkg(path.join(node.pkg.path, 'node_modules', node.pkg.name, 'package.json'))
        const labelLines = [
          printLabel(node),
          pkg.description,
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
    }),
  )
}

function printLabel (node: PackageNode) {
  let txt = `${node.pkg.name}@${node.pkg.version}`
  if (node.searched) {
    return chalk.yellow.bgBlack(txt)
  }
  if (node.saved === false) {
    txt += ` ${chalk.whiteBright.bgBlack('not saved')}`
  }
  return txt
}
