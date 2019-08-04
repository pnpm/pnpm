import { DEPENDENCIES_FIELDS } from '@pnpm/types'
import archy = require('archy')
import chalk from 'chalk'
import cliColumns = require('cli-columns')
import { DependenciesHierarchy, PackageNode } from 'dependencies-hierarchy'
import path = require('path')
import R = require('ramda')
import getPkgInfo from './getPkgInfo'

const sortPackages = R.sortBy(R.path(['pkg', 'name']) as (pkg: object) => R.Ord)

const DEV_DEP_ONLY_CLR = chalk.yellow
const PROD_DEP_CLR = (s: string) => s // just use the default color
const OPTIONAL_DEP_CLR = chalk.blue
const NOT_SAVED_DEP_CLR = chalk.red

const LEGEND = `Legend: ${PROD_DEP_CLR('production dependency')}, ${OPTIONAL_DEP_CLR('optional only')}, ${DEV_DEP_ONLY_CLR('dev only')}\n\n`

export default async function (
  project: {
    name?: string,
    version?: string,
    path: string,
  },
  tree: DependenciesHierarchy,
  opts: {
    alwaysPrintRootPackage: boolean,
    depth: number,
    long: boolean,
    search: boolean,
  },
) {
  if (
    !opts.alwaysPrintRootPackage &&
    (!tree.dependencies || !tree.dependencies.length) &&
    (!tree.devDependencies || !tree.devDependencies.length) &&
    (!tree.optionalDependencies || !tree.optionalDependencies.length) &&
    (!tree.unsavedDependencies || !tree.unsavedDependencies.length)
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
  let output = (opts.depth > -1 ? LEGEND : '') + label + '\n'
  const useColumns = opts.depth === 0 && opts.long === false && !opts.search
  for (let dependenciesField of [...DEPENDENCIES_FIELDS.sort(), 'unsavedDependencies']) {
    if (tree[dependenciesField] && tree[dependenciesField]!.length) {
      const depsLabel = chalk.cyanBright(
        dependenciesField !== 'unsavedDependencies'
          ? `${dependenciesField}:`
          : 'not saved (you should add these dependencies to package.json if you need them):'
      )
      output += `\n${depsLabel}\n`
      const gPkgColor = dependenciesField === 'unsavedDependencies' ? () => NOT_SAVED_DEP_CLR : getPkgColor
      if (useColumns && tree[dependenciesField].length > 10) {
        output += cliColumns(tree[dependenciesField].map(printLabel.bind(printLabel,  gPkgColor))) + '\n'
        continue
      }
      const data = await toArchyTree(gPkgColor, tree[dependenciesField]!, {
        long: opts.long,
        modules: path.join(project.path, 'node_modules'),
      })
      for (const d of data) {
        output += archy(d)
      }
    }
  }

  return output.replace(/\n$/, '')
}

type GetPkgColor = (node: PackageNode) => (s: string) => string

export async function toArchyTree (
  getPkgColor: GetPkgColor,
  entryNodes: PackageNode[],
  opts: {
    long: boolean,
    modules: string,
  },
): Promise<archy.Data[]> {
  return Promise.all(
    sortPackages(entryNodes).map(async (node) => {
      const nodes = await toArchyTree(getPkgColor, node.dependencies || [], opts)
      if (opts.long) {
        const pkg = await getPkgInfo(node)
        const labelLines = [
          printLabel(getPkgColor, node),
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
        label: printLabel(getPkgColor, node),
        nodes,
      }
    }),
  )
}

function printLabel (getPkgColor: GetPkgColor, node: PackageNode) {
  let color = getPkgColor(node)
  let txt = `${color(node.name)} ${chalk.gray(node.version)}`
  if (node.searched) {
    return chalk.bold.bgBlack(txt)
  }
  if (node.isPeer) {
    txt += ' peer'
  }
  return txt
}

function getPkgColor (node: PackageNode) {
  if (node.dev === true) return DEV_DEP_ONLY_CLR
  if (node.optional) return OPTIONAL_DEP_CLR
  return PROD_DEP_CLR
}
