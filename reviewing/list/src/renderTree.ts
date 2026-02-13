import path from 'path'
import { type PackageNode } from '@pnpm/reviewing.dependencies-hierarchy'
import { DEPENDENCIES_FIELDS, type DependenciesField } from '@pnpm/types'
import archy from 'archy'
import chalk from 'chalk'
import cliColumns from 'cli-columns'
import { sortBy, path as ramdaPath } from 'ramda'
import { type Ord } from 'ramda'
import { getPkgInfo } from './getPkgInfo.js'
import { type PackageDependencyHierarchy } from './types.js'

const sortPackages = sortBy(ramdaPath(['name']) as (pkg: PackageNode) => Ord)

const DEV_DEP_ONLY_CLR = chalk.yellow
const PROD_DEP_CLR = (s: string) => s // just use the default color
const OPTIONAL_DEP_CLR = chalk.blue
const NOT_SAVED_DEP_CLR = chalk.red

const LEGEND = `Legend: ${PROD_DEP_CLR('production dependency')}, ${OPTIONAL_DEP_CLR('optional only')}, ${DEV_DEP_ONLY_CLR('dev only')}\n\n`

export interface RenderTreeOptions {
  alwaysPrintRootPackage: boolean
  depth: number
  long: boolean
  search: boolean
  showExtraneous: boolean
}

export async function renderTree (
  packages: PackageDependencyHierarchy[],
  opts: RenderTreeOptions
): Promise<string> {
  const multiPeerPkgs = findMultiPeerPackages(packages)
  const output = (
    await Promise.all(packages.map(async (pkg) => renderTreeForPackage(pkg, opts, multiPeerPkgs)))
  )
    .filter(Boolean)
    .join('\n\n')
  return `${(opts.depth > -1 && output ? LEGEND : '')}${output}`
}

async function renderTreeForPackage (
  pkg: PackageDependencyHierarchy,
  opts: RenderTreeOptions,
  multiPeerPkgs: Set<string>
): Promise<string> {
  if (
    !opts.alwaysPrintRootPackage &&
    !pkg.dependencies?.length &&
    !pkg.devDependencies?.length &&
    !pkg.optionalDependencies?.length &&
    (!opts.showExtraneous || !pkg.unsavedDependencies?.length)
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

  if (pkg.private) {
    label += ' (PRIVATE)'
  }
  const useColumns = opts.depth === 0 && !opts.long && !opts.search
  const dependenciesFields: Array<DependenciesField | 'unsavedDependencies'> = [
    ...DEPENDENCIES_FIELDS.sort(),
  ]
  if (opts.showExtraneous) {
    dependenciesFields.push('unsavedDependencies')
  }
  const output = (await Promise.all(
    dependenciesFields.map(async (dependenciesField) => {
      if (pkg[dependenciesField]?.length) {
        const depsLabel = chalk.cyanBright(
          dependenciesField !== 'unsavedDependencies'
            ? `${dependenciesField}:`
            : 'not saved (you should add these dependencies to package.json if you need them):'
        )
        let output = `${depsLabel}\n`
        const gPkgColor = dependenciesField === 'unsavedDependencies' ? () => NOT_SAVED_DEP_CLR : getPkgColor
        if (useColumns && pkg[dependenciesField]!.length > 10) {
          output += cliColumns(pkg[dependenciesField]!.map((node) => printLabel(gPkgColor, multiPeerPkgs, node))) + '\n'
          return output
        }
        const data = await toArchyTree(gPkgColor, pkg[dependenciesField]!, {
          long: opts.long,
          modules: path.join(pkg.path, 'node_modules'),
          multiPeerPkgs,
        })
        for (const d of data) {
          output += archy(d)
        }
        return output
      }
      return null
    }))).filter(Boolean).join('\n')

  // eslint-disable-next-line regexp/no-unused-capturing-group
  return `${chalk.bold.underline(label)}\n\n${output}`.replace(/(\n)+$/, '')
}

type GetPkgColor = (node: PackageNode) => (s: string) => string

export async function toArchyTree (
  getPkgColor: GetPkgColor,
  entryNodes: PackageNode[],
  opts: {
    long: boolean
    modules: string
    multiPeerPkgs?: Set<string>
  }
): Promise<archy.Data[]> {
  return Promise.all(
    sortPackages(entryNodes).map(async (node) => {
      const nodes = await toArchyTree(getPkgColor, node.dependencies ?? [], opts)
      const labelLines: string[] = [
        printLabel(getPkgColor, opts.multiPeerPkgs, node),
      ]
      if (node.searchMessage) {
        labelLines.push(node.searchMessage)
      }
      if (opts.long) {
        const pkg = await getPkgInfo(node)
        if (pkg.description) {
          labelLines.push(pkg.description)
        }
        if (pkg.repository) {
          labelLines.push(pkg.repository)
        }
        if (pkg.homepage) {
          labelLines.push(pkg.homepage)
        }
        if (pkg.path) {
          labelLines.push(pkg.path)
        }
      }
      return {
        label: labelLines.join('\n'),
        nodes,
      }
    })
  )
}

function printLabel (getPkgColor: GetPkgColor, multiPeerPkgs: Set<string> | undefined, node: PackageNode): string {
  const color = getPkgColor(node)
  let txt: string
  if (node.alias !== node.name) {
    // When using npm: protocol alias, display as "alias npm:name@version"
    // Only add npm: prefix if version doesn't already contain @ (to avoid file:, link:, etc.)
    if (!node.version.includes('@')) {
      txt = `${color(node.alias)} ${chalk.gray(`npm:${node.name}@${node.version}`)}`
    } else {
      txt = `${color(node.alias)} ${chalk.gray(node.version)}`
    }
  } else {
    txt = `${color(node.name)} ${chalk.gray(node.version)}`
  }
  if (node.isPeer) {
    txt += ' peer'
  }
  if (node.isSkipped) {
    txt += ' skipped'
  }
  if (node.peersSuffixHash && multiPeerPkgs?.has(`${node.name}@${node.version}`)) {
    txt += chalk.dim(` #${node.peersSuffixHash}`)
  }
  if (node.deduped) {
    txt += chalk.dim(' deduped')
    if (node.dedupedDependenciesCount) {
      txt += chalk.dim(` (${node.dedupedDependenciesCount} dep${node.dedupedDependenciesCount === 1 ? '' : 's'} hidden)`)
    }
  }
  return node.searched ? chalk.bold(txt) : txt
}

function getPkgColor (node: PackageNode): (text: string) => string {
  if (node.dev === true) return DEV_DEP_ONLY_CLR
  if (node.optional) return OPTIONAL_DEP_CLR
  return PROD_DEP_CLR
}

/**
 * Walks all package trees and returns the set of `name@version` strings
 * that appear with more than one distinct `peersSuffixHash`.
 */
function findMultiPeerPackages (packages: PackageDependencyHierarchy[]): Set<string> {
  const hashesPerPkg = new Map<string, Set<string>>()

  function walk (nodes: PackageNode[]): void {
    for (const node of nodes) {
      if (node.peersSuffixHash) {
        const key = `${node.name}@${node.version}`
        let hashes = hashesPerPkg.get(key)
        if (hashes == null) {
          hashes = new Set()
          hashesPerPkg.set(key, hashes)
        }
        hashes.add(node.peersSuffixHash)
      }
      if (node.dependencies) {
        walk(node.dependencies)
      }
    }
  }

  for (const pkg of packages) {
    for (const field of DEPENDENCIES_FIELDS) {
      if (pkg[field]) {
        walk(pkg[field])
      }
    }
  }

  const result = new Set<string>()
  for (const [key, hashes] of hashesPerPkg) {
    if (hashes.size > 1) {
      result.add(key)
    }
  }
  return result
}
