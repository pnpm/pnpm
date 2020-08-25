import { PackageDependencyHierarchy } from './types'
import getPkgInfo from './getPkgInfo'
import { PackageNode } from 'dependencies-hierarchy'
import { DEPENDENCIES_FIELDS } from '@pnpm/types'
import path = require('path')
import archy = require('archy')
import chalk = require('chalk')
import cliColumns = require('cli-columns')
import R = require('ramda')

const sortPackages = R.sortBy(R.path(['name']) as (pkg: object) => R.Ord)

const DEV_DEP_ONLY_CLR = chalk.yellow
const PROD_DEP_CLR = (s: string) => s // just use the default color
const OPTIONAL_DEP_CLR = chalk.blue
const NOT_SAVED_DEP_CLR = chalk.red

const LEGEND = `Legend: ${PROD_DEP_CLR('production dependency')}, ${OPTIONAL_DEP_CLR('optional only')}, ${DEV_DEP_ONLY_CLR('dev only')}\n\n`

export default async function (
  packages: PackageDependencyHierarchy[],
  opts: {
    alwaysPrintRootPackage: boolean
    depth: number
    long: boolean
    search: boolean
  }
) {
  const output = (
    await Promise.all(packages.map((pkg) => renderTreeForPackage(pkg, opts)))
  )
    .filter(Boolean)
    .join('\n\n')
  return `${(opts.depth > -1 && output ? LEGEND : '')}${output}`
}

async function renderTreeForPackage (
  pkg: PackageDependencyHierarchy,
  opts: {
    alwaysPrintRootPackage: boolean
    depth: number
    long: boolean
    search: boolean
  }
) {
  if (
    !opts.alwaysPrintRootPackage &&
    !pkg.dependencies?.length &&
    !pkg.devDependencies?.length &&
    !pkg.optionalDependencies?.length &&
    !pkg.unsavedDependencies?.length
  ) return ''

  let label = ''
  if (pkg.name) {
    label += pkg.name
    if (pkg.version) {
      label += `@${pkg.version}`
    }
    label += ' '
  }
  label += pkg.path
  let output = `${label}\n`
  const useColumns = opts.depth === 0 && !opts.long && !opts.search
  for (const dependenciesField of [...DEPENDENCIES_FIELDS.sort(), 'unsavedDependencies']) {
    if (pkg[dependenciesField]?.length) {
      const depsLabel = chalk.cyanBright(
        dependenciesField !== 'unsavedDependencies'
          ? `${dependenciesField}:`
          : 'not saved (you should add these dependencies to package.json if you need them):'
      )
      output += `\n${depsLabel}\n`
      const gPkgColor = dependenciesField === 'unsavedDependencies' ? () => NOT_SAVED_DEP_CLR : getPkgColor
      if (useColumns && pkg[dependenciesField].length > 10) {
        output += cliColumns(pkg[dependenciesField].map(printLabel.bind(printLabel, gPkgColor))) + '\n'
        continue
      }
      const data = await toArchyTree(gPkgColor, pkg[dependenciesField]!, {
        long: opts.long,
        modules: path.join(pkg.path, 'node_modules'),
      })
      for (const d of data) {
        output += archy(d)
      }
    }
  }

  return output.replace(/\n$/, '')
}

type GetPkgColor = (node: PackageNode) => (s: string) => string

export function toArchyTree (
  getPkgColor: GetPkgColor,
  entryNodes: PackageNode[],
  opts: {
    long: boolean
    modules: string
  }
): Promise<archy.Data[]> {
  return Promise.all(
    sortPackages(entryNodes).map(async (node) => {
      const nodes = await toArchyTree(getPkgColor, node.dependencies ?? [], opts)
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
    })
  )
}

function printLabel (getPkgColor: GetPkgColor, node: PackageNode) {
  const color = getPkgColor(node)
  let txt = `${color(node.name)} ${chalk.gray(node.version)}`
  if (node.isPeer) {
    txt += ' peer'
  }
  if (node.isSkipped) {
    txt += ' skipped'
  }
  return node.searched ? chalk.bold(txt) : txt
}

function getPkgColor (node: PackageNode) {
  if (node.dev === true) return DEV_DEP_ONLY_CLR
  if (node.optional) return OPTIONAL_DEP_CLR
  return PROD_DEP_CLR
}
