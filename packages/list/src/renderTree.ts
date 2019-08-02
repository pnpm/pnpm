import { DEPENDENCIES_FIELDS } from '@pnpm/types'
import archy = require('archy')
import chalk from 'chalk'
import { DependenciesHierarchy, PackageNode } from 'dependencies-hierarchy'
import path = require('path')
import R = require('ramda')
import getPkgInfo from './getPkgInfo'

const sortPackages = R.sortBy(R.path(['pkg', 'name']) as (pkg: object) => R.Ord)

const LEGEND = `Legend:\n  white - production dependency\n  ${chalk.blue('blue')} - optional only dependency\n  ${chalk.grey('grey')} - dev only dependency\n\n`

export default async function (
  project: {
    name?: string,
    version?: string,
    path: string,
  },
  tree: DependenciesHierarchy,
  opts: {
    alwaysPrintRootPackage: boolean,
    long: boolean,
  },
) {
  if (
    !opts.alwaysPrintRootPackage &&
    (!tree.dependencies || !tree.dependencies.length) &&
    (!tree.devDependencies || !tree.devDependencies.length) &&
    (!tree.optionalDependencies || !tree.optionalDependencies.length)
  ) return ''

  let label = ''
  if (project.name) {
    label += project.name
    if (project.version) {
      label += `@${project.version}`
    }
    label += ' '
  }
  label += project.path
  let output = LEGEND
  for (let dependenciesField of DEPENDENCIES_FIELDS.sort()) {
    if (tree[dependenciesField]) {
      output += archy({
        label: chalk.cyanBright(`${dependenciesField}:`),
        nodes: await toArchyTree(tree[dependenciesField]!, {
          long: opts.long,
          modules: path.join(project.path, 'node_modules'),
        })
      })
    }
  }

  return output.replace(/\n$/, '')
}

export async function toArchyTree (
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
        const pkg = await getPkgInfo(node.pkg)
        const labelLines = [
          printLabel(node),
          pkg.description,
        ]
        if (pkg.repository) {
          labelLines.push(pkg.repository)
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
  if (node.pkg.dev === true) {
    txt = chalk.grey(txt)
  } else if (node.pkg.optional === true) {
    txt = chalk.blue(txt)
  }
  if (node.searched) {
    return chalk.bgYellow(txt)
  }
  if (node.saved === false) {
    txt += ` ${chalk.whiteBright.bgBlack('not saved')}`
  }
  return txt
}
