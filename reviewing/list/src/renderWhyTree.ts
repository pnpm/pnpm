import { type WhyPackageResult, type WhyDependant } from '@pnpm/reviewing.dependencies-hierarchy'
import { renderTree as renderArchyTree, type TreeNode } from '@pnpm/text.tree-renderer'
import chalk from 'chalk'
import { collectHashes, DEDUPED_LABEL, filterMultiPeerEntries, nameAtVersion, peerHashSuffix } from './peerVariants.js'

function plainNameAtVersion (name: string, version: string): string {
  return version ? `${name}@${version}` : name
}

export function renderWhyTree (results: WhyPackageResult[]): string {
  if (results.length === 0) return ''

  const multiPeerPkgs = findMultiPeerPackages(results)

  const trees = results
    .map((result) => {
      const rootLabelParts = [chalk.bold(nameAtVersion(result.name, result.version)) +
        peerHashSuffix(result.name, result.version, result.peersSuffixHash, multiPeerPkgs)]
      if (result.searchMessage) {
        rootLabelParts.push(result.searchMessage)
      }
      const rootLabel = rootLabelParts.join('\n')
      if (result.dependants.length === 0) {
        return rootLabel
      }
      const childNodes = dependantsToTreeNodes(result.dependants, multiPeerPkgs)
      const tree: TreeNode = { label: rootLabel, nodes: childNodes }
      return renderArchyTree(tree, { treeChars: chalk.dim }).replace(/\n+$/, '')
    })
    .join('\n\n')

  const summary = whySummary(results)
  return summary ? `${trees}\n\n${summary}` : trees
}

function whySummary (results: WhyPackageResult[]): string {
  if (results.length === 0) return ''
  const versions = new Set(results.map(r => r.version))
  const parts: string[] = [`${versions.size} version${versions.size === 1 ? '' : 's'}`]
  if (results.length > versions.size) {
    parts.push(`${results.length} instances`)
  }
  return chalk.dim(`Found ${parts.join(', ')} of ${results[0].name}`)
}

function findMultiPeerPackages (results: WhyPackageResult[]): Map<string, number> {
  const hashesPerPkg = new Map<string, Set<string>>()

  function walkDependants (dependants: WhyDependant[]): void {
    for (const dep of dependants) {
      collectHashes(hashesPerPkg, dep.name, dep.version, dep.peersSuffixHash)
      if (dep.dependants) {
        walkDependants(dep.dependants)
      }
    }
  }

  for (const result of results) {
    collectHashes(hashesPerPkg, result.name, result.version, result.peersSuffixHash)
    walkDependants(result.dependants)
  }

  return filterMultiPeerEntries(hashesPerPkg)
}

function dependantsToTreeNodes (dependants: WhyDependant[], multiPeerPkgs: Map<string, number>): TreeNode[] {
  return dependants.map((dep) => {
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

    const nodes = dep.dependants ? dependantsToTreeNodes(dep.dependants, multiPeerPkgs) : []
    return { label, nodes }
  })
}

export function renderWhyJson (results: WhyPackageResult[]): string {
  return JSON.stringify(results, null, 2)
}

export function renderWhyParseable (results: WhyPackageResult[]): string {
  const lines: string[] = []
  for (const result of results) {
    collectPaths(result.dependants, [plainNameAtVersion(result.name, result.version)], lines)
  }
  return lines.join('\n')
}

function collectPaths (dependants: WhyDependant[], currentPath: string[], lines: string[]): void {
  for (const dep of dependants) {
    const newPath = [...currentPath, plainNameAtVersion(dep.name, dep.version)]
    if (dep.dependants && dep.dependants.length > 0) {
      collectPaths(dep.dependants, newPath, lines)
    } else {
      // Leaf node (importer) — reverse to show importer first
      lines.push([...newPath].reverse().join(' > '))
    }
  }
}
