import path from 'path'
import { type DependencyNode } from '@pnpm/reviewing.dependencies-hierarchy'
import { renderTree as renderArchyTree, type TreeNode, type TreeNodeGroup } from '@pnpm/text.tree-renderer'
import { DEPENDENCIES_FIELDS, type DependenciesField } from '@pnpm/types'
import { lexCompare } from '@pnpm/util.lex-comparator'
import chalk from 'chalk'
import { getPkgInfo } from './getPkgInfo.js'
import { collectHashes, DEDUPED_LABEL, filterMultiPeerEntries, nameAtVersion, peerHashSuffix } from './peerVariants.js'
import { type PackageDependencyHierarchy } from './types.js'

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
  showSummary?: boolean
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
  const legend = opts.depth > -1 && output ? LEGEND : ''
  const summary = opts.showSummary && opts.depth > -1 && output ? `\n\n${listSummary(packages)}` : ''
  return `${legend}${output}${summary}`
}

async function renderTreeForPackage (
  pkg: PackageDependencyHierarchy,
  opts: RenderTreeOptions,
  multiPeerPkgs: Map<string, number>
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
    label += nameAtVersion(pkg.name, pkg.version ?? '')
    label += ' '
  }
  label += chalk.dim(pkg.path)

  if (pkg.private) {
    label += chalk.dim(' (PRIVATE)')
  }
  const dependenciesFields: Array<DependenciesField | 'unsavedDependencies'> = [
    ...DEPENDENCIES_FIELDS.sort(),
  ]
  if (opts.showExtraneous) {
    dependenciesFields.push('unsavedDependencies')
  }
  const childNodes: TreeNodeGroup[] = (await Promise.all(
    dependenciesFields.map(async (dependenciesField) => {
      if (!pkg[dependenciesField]?.length) return null
      const depsLabel = chalk.cyanBright(
        dependenciesField !== 'unsavedDependencies'
          ? `${dependenciesField}:`
          : 'not saved (you should add these dependencies to package.json if you need them):'
      )
      const gPkgColor = dependenciesField === 'unsavedDependencies' ? () => NOT_SAVED_DEP_CLR : getPkgColor
      const depNodes = await toArchyTree(gPkgColor, pkg[dependenciesField]!, {
        long: opts.long,
        modules: path.join(pkg.path, 'node_modules'),
        multiPeerPkgs,
      })
      return { group: depsLabel, nodes: depNodes } as TreeNodeGroup
    })
  )).filter((n): n is TreeNodeGroup => n != null)

  const rootLabel = chalk.bold(label)
  if (childNodes.length === 0) {
    return rootLabel
  }
  const tree: TreeNode = { label: rootLabel, nodes: childNodes }
  return renderArchyTree(tree, { treeChars: chalk.dim }).trimEnd()
}

type GetPkgColor = (node: DependencyNode) => (s: string) => string

export async function toArchyTree (
  getPkgColor: GetPkgColor,
  entryNodes: DependencyNode[],
  opts: {
    long: boolean
    modules: string
    multiPeerPkgs?: Map<string, number>
  }
): Promise<TreeNode[]> {
  const sorted = [...entryNodes].sort((a, b) => lexCompare(a.name, b.name))
  return Promise.all(
    sorted.map(async (node) => {
      const nodes: TreeNode[] = node.deduped
        ? []
        : await toArchyTree(getPkgColor, node.dependencies ?? [], opts)
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

function printLabel (getPkgColor: GetPkgColor, multiPeerPkgs: Map<string, number> | undefined, node: DependencyNode): string {
  const color = getPkgColor(node)
  let txt: string
  if (node.alias !== node.name) {
    // When using npm: protocol alias, display as "alias@npm:name@version"
    // Only add npm: prefix if version doesn't already contain @ (to avoid file:, link:, etc.)
    if (!node.version.includes('@')) {
      txt = `${color(node.alias)}${chalk.gray(`@npm:${node.name}@${node.version}`)}`
    } else {
      txt = `${color(node.alias)}${chalk.gray(`@${node.version}`)}`
    }
  } else {
    txt = nameAtVersion(node.name, node.version, color)
  }
  if (node.isPeer) {
    txt += ' peer'
  }
  if (node.isSkipped) {
    txt += ' skipped'
  }
  if (multiPeerPkgs) {
    txt += peerHashSuffix(node, multiPeerPkgs)
  }
  if (node.deduped) {
    txt += DEDUPED_LABEL
  }
  return node.searched ? chalk.bold(txt) : txt
}

function getPkgColor (node: DependencyNode): (text: string) => string {
  if (node.dev === true) return DEV_DEP_ONLY_CLR
  if (node.optional) return OPTIONAL_DEP_CLR
  return PROD_DEP_CLR
}

/**
 * Walks all package trees and returns the set of `name@version` strings
 * that appear with more than one distinct `peersSuffixHash`.
 */
function findMultiPeerPackages (packages: PackageDependencyHierarchy[]): Map<string, number> {
  const hashesPerPkg = new Map<string, Set<string>>()

  function walk (nodes: DependencyNode[]): void {
    for (const node of nodes) {
      collectHashes(hashesPerPkg, node)
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

  return filterMultiPeerEntries(hashesPerPkg)
}

function listSummary (packages: PackageDependencyHierarchy[]): string {
  let total = 0

  function walk (nodes: DependencyNode[]): void {
    for (const node of nodes) {
      total++
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

  const parts: string[] = [`${total} package${total === 1 ? '' : 's'}`]
  if (packages.length > 1) {
    parts.push(`${packages.length} projects`)
  }
  return chalk.dim(parts.join(' in '))
}
