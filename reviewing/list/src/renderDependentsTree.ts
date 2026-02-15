import { type DependentsTree, type DependentNode } from '@pnpm/reviewing.dependencies-hierarchy'
import { renderTree as renderArchyTree, type TreeNode } from '@pnpm/text.tree-renderer'
import chalk from 'chalk'
import { collectHashes, DEDUPED_LABEL, filterMultiPeerEntries, nameAtVersion, peerHashSuffix } from './peerVariants.js'
import { getPkgInfo } from './getPkgInfo.js'

export async function renderDependentsTree (trees: DependentsTree[], opts: { long: boolean }): Promise<string> {
  if (trees.length === 0) return ''

  const multiPeerPkgs = findMultiPeerPackages(trees)

  const output = (
    await Promise.all(trees.map(async (result) => {
      const rootLabelParts = [chalk.bold(nameAtVersion(result.name, result.version)) +
        peerHashSuffix(result, multiPeerPkgs)]
      if (result.searchMessage) {
        rootLabelParts.push(result.searchMessage)
      }
      if (opts.long && result.path) {
        const pkg = await getPkgInfo({ name: result.name, version: result.version, path: result.path, alias: undefined })
        if (pkg.description) {
          rootLabelParts.push(pkg.description)
        }
        if (pkg.repository) {
          rootLabelParts.push(pkg.repository)
        }
        if (pkg.homepage) {
          rootLabelParts.push(pkg.homepage)
        }
        rootLabelParts.push(pkg.path)
      }
      const rootLabel = rootLabelParts.join('\n')
      if (result.dependents.length === 0) {
        return rootLabel
      }
      const childNodes = dependentsToTreeNodes(result.dependents, multiPeerPkgs)
      const tree: TreeNode = { label: rootLabel, nodes: childNodes }
      return renderArchyTree(tree, { treeChars: chalk.dim }).replace(/\n+$/, '')
    }))
  ).join('\n\n')

  const summary = whySummary(trees)
  return summary ? `${output}\n\n${summary}` : output
}

function whySummary (trees: DependentsTree[]): string {
  if (trees.length === 0) return ''

  const byName = new Map<string, { versions: Set<string>, count: number }>()
  for (const tree of trees) {
    let entry = byName.get(tree.name)
    if (entry == null) {
      entry = { versions: new Set<string>(), count: 0 }
      byName.set(tree.name, entry)
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

function dependentsToTreeNodes (dependents: DependentNode[], multiPeerPkgs: Map<string, number>): TreeNode[] {
  return dependents.map((dep) => {
    let label: string
    if (dep.depField != null) {
      // This is an importer (leaf node)
      label = chalk.bold(nameAtVersion(dep.name, dep.version)) + ` ${chalk.dim(`(${dep.depField})`)}`
    } else {
      label = nameAtVersion(dep.name, dep.version)
      label += peerHashSuffix(dep, multiPeerPkgs)
    }

    if (dep.circular) {
      label += chalk.dim(' [circular]')
    }
    if (dep.deduped) {
      label += DEDUPED_LABEL
    }

    const nodes = dep.dependents ? dependentsToTreeNodes(dep.dependents, multiPeerPkgs) : []
    return { label, nodes }
  })
}

export async function renderDependentsJson (trees: DependentsTree[], opts: { long: boolean }): Promise<string> {
  if (!opts.long) {
    return JSON.stringify(trees, null, 2)
  }
  const enriched = await Promise.all(trees.map(async (result) => {
    if (!result.path) return result
    const pkg = await getPkgInfo({ name: result.name, version: result.version, path: result.path, alias: undefined })
    return {
      ...result,
      description: pkg.description,
      repository: pkg.repository,
      homepage: pkg.homepage,
    }
  }))
  return JSON.stringify(enriched, null, 2)
}

export function renderDependentsParseable (trees: DependentsTree[], opts: { long: boolean }): string {
  const lines: string[] = []
  for (const result of trees) {
    const rootSegment = opts.long && result.path
      ? `${result.path}:${plainNameAtVersion(result.name, result.version)}`
      : plainNameAtVersion(result.name, result.version)
    collectPaths(result.dependents, [rootSegment], lines)
  }
  return lines.join('\n')
}

function collectPaths (dependents: DependentNode[], currentPath: string[], lines: string[]): void {
  for (const dep of dependents) {
    const newPath = [...currentPath, plainNameAtVersion(dep.name, dep.version)]
    if (dep.dependents && dep.dependents.length > 0) {
      collectPaths(dep.dependents, newPath, lines)
    } else {
      // Leaf node (importer) — reverse to show importer first
      lines.push([...newPath].reverse().join(' > '))
    }
  }
}

function plainNameAtVersion (name: string, version: string): string {
  return version ? `${name}@${version}` : name
}
