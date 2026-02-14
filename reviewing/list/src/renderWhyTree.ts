import { type WhyPackageResult, type WhyDependant } from '@pnpm/reviewing.dependencies-hierarchy'
import { renderTree as renderArchyTree, type TreeNode } from '@pnpm/text.tree-renderer'
import chalk from 'chalk'

export function renderWhyTree (results: WhyPackageResult[]): string {
  if (results.length === 0) return ''

  return results
    .map((result) => {
      const rootLabel = chalk.bold(
        result.version ? `${result.name}@${result.version}` : result.name
      )
      if (result.dependants.length === 0) {
        return rootLabel
      }
      const childNodes = dependantsToTreeNodes(result.dependants)
      const tree: TreeNode = { label: rootLabel, nodes: childNodes }
      return renderArchyTree(tree, { treeChars: chalk.dim }).replace(/\n+$/, '')
    })
    .join('\n\n')
}

function dependantsToTreeNodes (dependants: WhyDependant[]): TreeNode[] {
  return dependants.map((dep) => {
    let label: string
    if (dep.depField != null) {
      // This is an importer (leaf node)
      const fieldLabel = dep.depField === 'dependencies' ? '' : ` ${chalk.dim(`(${dep.depField})`)}`
      label = dep.version
        ? `${dep.name}@${dep.version}${fieldLabel}`
        : `${dep.name}${fieldLabel}`
    } else {
      label = dep.version ? `${dep.name}@${dep.version}` : dep.name
    }

    if (dep.circular) {
      label += chalk.dim(' [circular]')
    }
    if (dep.deduped) {
      label += chalk.dim(' [deduped]')
    }

    const nodes = dep.dependants ? dependantsToTreeNodes(dep.dependants) : []
    return { label, nodes }
  })
}

export function renderWhyJson (results: WhyPackageResult[]): string {
  return JSON.stringify(results, null, 2)
}

export function renderWhyParseable (results: WhyPackageResult[]): string {
  const lines: string[] = []
  for (const result of results) {
    const rootLabel = result.version ? `${result.name}@${result.version}` : result.name
    collectPaths(result.dependants, [rootLabel], lines)
  }
  return lines.join('\n')
}

function collectPaths (dependants: WhyDependant[], currentPath: string[], lines: string[]): void {
  for (const dep of dependants) {
    const depLabel = dep.version ? `${dep.name}@${dep.version}` : dep.name
    const newPath = [...currentPath, depLabel]
    if (dep.dependants && dep.dependants.length > 0) {
      collectPaths(dep.dependants, newPath, lines)
    } else {
      // Leaf node (importer) — reverse to show importer first
      lines.push([...newPath].reverse().join(' > '))
    }
  }
}
