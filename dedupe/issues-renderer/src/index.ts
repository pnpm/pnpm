import {
  type ResolutionChange,
  type DedupeCheckIssues,
  type ResolutionChangesByAlias,
  type SnapshotsChanges,
} from '@pnpm/dedupe.types'
import { renderTree, type TreeNode } from '@pnpm/text.tree-renderer'
import chalk from 'chalk'

export function renderDedupeCheckIssues (dedupeCheckIssues: DedupeCheckIssues): string {
  const importersReport = report(dedupeCheckIssues.importerIssuesByImporterId)
  const packagesReport = report(dedupeCheckIssues.packageIssuesByDepPath)

  const lines = []
  if (importersReport !== '') {
    lines.push(chalk.blueBright.underline('Importers'))
    lines.push(importersReport)
    lines.push('')
  }
  if (packagesReport !== '') {
    lines.push(chalk.blueBright.underline('Packages'))
    lines.push(packagesReport)
    lines.push('')
  }

  return lines.join('\n')
}

/**
 * Render snapshot changes. Expected to return an empty string for no changes.
 */
function report (snapshotChanges: SnapshotsChanges): string {
  return [
    ...Object.entries(snapshotChanges.updated).map(([alias, updates]) => renderTree(toArchy(alias, updates))),
    ...snapshotChanges.added.map((id) => `${chalk.green('+')} ${id}`),
    ...snapshotChanges.removed.map((id) => `${chalk.red('-')} ${id}`),
  ].join('\n')
}

function toArchy (name: string, issue: ResolutionChangesByAlias): TreeNode {
  return {
    label: name,
    nodes: Object.entries(issue).map(([alias, change]) => toArchyResolution(alias, change)),
  }
}

function toArchyResolution (alias: string, change: ResolutionChange): TreeNode {
  switch (change.type) {
  case 'added':
    return { label: `${chalk.green('+')} ${alias} ${chalk.gray(change.next)}` }
  case 'removed':
    return { label: `${chalk.red('-')} ${alias} ${chalk.gray(change.prev)}` }
  case 'updated':
    return { label: `${alias} ${chalk.red(change.prev)} ${chalk.gray('→')} ${chalk.green(change.next)}` }
  }
}
