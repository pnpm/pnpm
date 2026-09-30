import type { DependentNode, DependentsTree } from '@pnpm/deps.inspection.tree-builder'
import { renderTree as renderArchyTree, type TreeNode } from '@pnpm/text.tree-renderer'
import chalk from 'chalk'

import { getPkgInfo } from './getPkgInfo.js'
import { collectHashes, DEDUPED_LABEL, filterMultiPeerEntries, nameAtVersion, peerHashSuffix } from './peerVariants.js'

export async function renderDependentsTree (trees: DependentsTree[], opts: { long: boolean, depth?: number }): Promise<string> {
  if (trees.length === 0) return ''

  const multiPeerPkgs = findMultiPeerPackages(trees)

  const output = (
    await Promise.all(trees.map(async (result) => renderDependentsTreeRoot(result, { ...opts, multiPeerPkgs })))
  ).join('\n\n')

  const summary = whySummary(trees)
  return summary ? `${output}\n\n${summary}` : output
}

async function renderDependentsTreeRoot (
  result: DependentsTree,
  opts: { long: boolean, depth?: number, multiPeerPkgs: Map<string, number> }
): Promise<string> {
  const rootLabel = await buildRootLabel(result, opts)
  if (result.dependents.length === 0) {
    return rootLabel
  }
  const childNodes = dependentsToTreeNodes(result.dependents, opts.multiPeerPkgs, 0, opts.depth)
  const tree: TreeNode = { label: rootLabel, nodes: childNodes }
  return trimTrailingNewlines(renderArchyTree(tree, { treeChars: chalk.dim }))
}

async function buildRootLabel (
  result: DependentsTree,
  opts: { long: boolean, multiPeerPkgs: Map<string, number> }
): Promise<string> {
  const rootLabelParts = [chalk.bold(nameAtVersion(result.displayName ?? result.name, result.version)) +
    peerHashSuffix(result, opts.multiPeerPkgs)]
  if (result.searchMessage) {
    rootLabelParts.push(result.searchMessage)
  }
  if (opts.long && result.path) {
    const pkg = await getPkgInfo({ name: result.name, version: result.version, path: result.path, alias: undefined })
    rootLabelParts.push(...[pkg.description, pkg.repository, pkg.homepage].filter(Boolean) as string[])
    rootLabelParts.push(pkg.path)
  }
  return rootLabelParts.join('\n')
}

function whySummary (trees: DependentsTree[]): string {
  if (trees.length === 0) return ''

  const byName = new Map<string, { versions: Set<string>, count: number }>()
  for (const tree of trees) {
    const displayedName = tree.displayName ?? tree.name
    let entry = byName.get(displayedName)
    if (entry == null) {
      entry = { versions: new Set<string>(), count: 0 }
      byName.set(displayedName, entry)
    }
    entry.versions.add(tree.version)
    entry.count++
  }

  const lines: string[] = []
  for (const [name, info] of byName) {
    const parts: string[] = [`${info.versions.size} version${info.versions.size === 1 ? '' : 's'}`]
    if (info.count > info.versions.size) {
      parts.push(`${info.count} instances`)
    }
    lines.push(`Found ${parts.join(', ')} of ${name}`)
  }
  return chalk.dim(lines.join('\n'))
}

function findMultiPeerPackages (trees: DependentsTree[]): Map<string, number> {
  const hashesPerPkg = new Map<string, Set<string>>()

  function walkDependents (dependents: DependentNode[]): void {
    for (const dep of dependents) {
      collectHashes(hashesPerPkg, dep)
      if (dep.dependents) {
        walkDependents(dep.dependents)
      }
    }
  }

  for (const tree of trees) {
    collectHashes(hashesPerPkg, tree)
    walkDependents(tree.dependents)
  }

  return filterMultiPeerEntries(hashesPerPkg)
}

function dependentsToTreeNodes (dependents: DependentNode[], multiPeerPkgs: Map<string, number>, currentDepth: number, maxDepth?: number): TreeNode[] {
  return dependents.map((dep) => {
    const label = dependentLabel(dep, multiPeerPkgs)
    const atDepthLimit = maxDepth != null && currentDepth + 1 >= maxDepth
    const nodes = dep.dependents && !atDepthLimit
      ? dependentsToTreeNodes(dep.dependents, multiPeerPkgs, currentDepth + 1, maxDepth)
      : []
    return { label, nodes }
  })
}

function dependentLabel (dep: DependentNode, multiPeerPkgs: Map<string, number>): string {
  const displayedName = dep.displayName ?? dep.name
  let label: string
  if (dep.depField != null) {
    // This is an importer (leaf node)
    label = chalk.bold(nameAtVersion(displayedName, dep.version)) + ` ${chalk.dim(`(${dep.depField})`)}`
  } else {
    label = nameAtVersion(displayedName, dep.version)
    label += peerHashSuffix(dep, multiPeerPkgs)
  }

  if (dep.circular) {
    label += chalk.dim(' [circular]')
  }
  if (dep.deduped) {
    label += DEDUPED_LABEL
  }
  return label
}

export async function renderDependentsJson (trees: DependentsTree[], opts: { long: boolean, depth?: number }): Promise<string> {
  let data: DependentsTree[] | Array<DependentsTree & { description?: string, repository?: string, homepage?: string }> = trees
  if (opts.long) {
    data = await Promise.all(trees.map(async (result) => {
      if (!result.path) return result
      const pkg = await getPkgInfo({ name: result.name, version: result.version, path: result.path, alias: undefined })
      return {
        ...result,
        description: pkg.description,
        repository: pkg.repository,
        homepage: pkg.homepage,
      }
    }))
  }
  if (opts.depth != null) {
    data = data.map((tree) => ({
      ...tree,
      dependents: truncateDependents(tree.dependents, 0, opts.depth!),
    }))
  }
  return JSON.stringify(data, null, 2)
}

export function renderDependentsParseable (trees: DependentsTree[], opts: { long: boolean, depth?: number }): string {
  const lines: string[] = []
  for (const result of trees) {
    const displayedName = result.displayName ?? result.name
    const rootSegment = opts.long && result.path
      ? `${result.path}:${plainNameAtVersion(displayedName, result.version)}`
      : plainNameAtVersion(displayedName, result.version)
    collectPaths(result.dependents, [rootSegment], lines, 0, opts.depth)
  }
  return lines.join('\n')
}

function collectPaths (dependents: DependentNode[], currentPath: string[], lines: string[], currentDepth: number, maxDepth?: number): void {
  for (const dep of dependents) {
    const newPath = [...currentPath, plainNameAtVersion(dep.displayName ?? dep.name, dep.version)]
    const atDepthLimit = maxDepth != null && currentDepth + 1 >= maxDepth
    if (dep.dependents && dep.dependents.length > 0 && !atDepthLimit) {
      collectPaths(dep.dependents, newPath, lines, currentDepth + 1, maxDepth)
    } else {
      // Leaf node (importer or depth-limited) — reverse to show importer first
      lines.push([...newPath].reverse().join(' > '))
    }
  }
}

function truncateDependents (dependents: DependentNode[], currentDepth: number, maxDepth: number): DependentNode[] {
  return dependents.map((dep) => {
    if (dep.dependents && currentDepth + 1 < maxDepth) {
      return { ...dep, dependents: truncateDependents(dep.dependents, currentDepth + 1, maxDepth) }
    }
    const { dependents: _, ...rest } = dep
    return rest
  })
}

function plainNameAtVersion (name: string, version: string): string {
  return version ? `${name}@${version}` : name
}

function trimTrailingNewlines (text: string): string {
  let end = text.length
  while (end > 0 && text.charCodeAt(end - 1) === 10) end--
  return end === text.length ? text : text.slice(0, end)
}
