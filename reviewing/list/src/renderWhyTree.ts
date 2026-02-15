import { type DependentsTree, type Dependent } from '@pnpm/reviewing.dependencies-hierarchy'
import { renderTree as renderArchyTree, type TreeNode } from '@pnpm/text.tree-renderer'
import chalk from 'chalk'
import { collectHashes, DEDUPED_LABEL, filterMultiPeerEntries, nameAtVersion, peerHashSuffix } from './peerVariants.js'
import { getPkgInfo } from './getPkgInfo.js'

function plainNameAtVersion (name: string, version: string): string {
  return version ? `${name}@${version}` : name
}

export async function renderWhyTree (results: DependentsTree[], opts: { long: boolean }): Promise<string> {
  if (results.length === 0) return ''

  const multiPeerPkgs = findMultiPeerPackages(results)

  const trees = (
    await Promise.all(results.map(async (result) => {
      const rootLabelParts = [chalk.bold(nameAtVersion(result.name, result.version)) +
        peerHashSuffix(result.name, result.version, result.peersSuffixHash, multiPeerPkgs)]
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

  const summary = whySummary(results)
  return summary ? `${trees}\n\n${summary}` : trees
}

function whySummary (results: DependentsTree[]): string {
  if (results.length === 0) return ''
  const versions = new Set(results.map(r => r.version))
  const parts: string[] = [`${versions.size} version${versions.size === 1 ? '' : 's'}`]
  if (results.length > versions.size) {
    parts.push(`${results.length} instances`)
  }
  return chalk.dim(`Found ${parts.join(', ')} of ${results[0].name}`)
}

function findMultiPeerPackages (results: DependentsTree[]): Map<string, number> {
  const hashesPerPkg = new Map<string, Set<string>>()

  function walkDependents (dependents: Dependent[]): void {
    for (const dep of dependents) {
      collectHashes(hashesPerPkg, dep.name, dep.version, dep.peersSuffixHash)
      if (dep.dependents) {
        walkDependents(dep.dependents)
      }
    }
  }

  for (const result of results) {
    collectHashes(hashesPerPkg, result.name, result.version, result.peersSuffixHash)
    walkDependents(result.dependents)
  }

  return filterMultiPeerEntries(hashesPerPkg)
}

function dependentsToTreeNodes (dependents: Dependent[], multiPeerPkgs: Map<string, number>): TreeNode[] {
  return dependents.map((dep) => {
    let label: string
    if (dep.depField != null) {
      // This is an importer (leaf node)
      label = chalk.bold(nameAtVersion(dep.name, dep.version)) + ` ${chalk.dim(`(${dep.depField})`)}`
    } else {
      label = nameAtVersion(dep.name, dep.version)
      label += peerHashSuffix(dep.name, dep.version, dep.peersSuffixHash, multiPeerPkgs)
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

export async function renderWhyJson (results: DependentsTree[], opts: { long: boolean }): Promise<string> {
  if (!opts.long) {
    return JSON.stringify(results, null, 2)
  }
  const enriched = await Promise.all(results.map(async (result) => {
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

export function renderWhyParseable (results: DependentsTree[], opts: { long: boolean }): string {
  const lines: string[] = []
  for (const result of results) {
    const rootSegment = opts.long && result.path
      ? `${result.path}:${plainNameAtVersion(result.name, result.version)}`
      : plainNameAtVersion(result.name, result.version)
    collectPaths(result.dependents, [rootSegment], lines)
  }
  return lines.join('\n')
}

function collectPaths (dependents: Dependent[], currentPath: string[], lines: string[]): void {
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
