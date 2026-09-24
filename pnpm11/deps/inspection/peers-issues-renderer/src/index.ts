import type { BadPeerDependencyIssue, PeerDependencyIssues, PeerDependencyIssuesByProjects } from '@pnpm/types'
import chalk from 'chalk'

export function renderPeerIssues (issuesByProjects: PeerDependencyIssuesByProjects): string {
  const projects: Array<[string, string[]]> = []
  for (const [projectId, projectIssues] of Object.entries(issuesByProjects)) {
    const sections = renderProjectSections(projectIssues)
    if (sections.length > 0) {
      projects.push([projectId, sections])
    }
  }
  if (projects.length === 1 && projects[0][0] === '.') {
    return projects[0][1].join('\n\n')
  }
  return projects
    .map(([projectId, sections]) => `${chalk.underline(projectId)}\n${sections.map(indent).join('\n\n')}`)
    .join('\n\n')
}

function renderProjectSections ({ bad, missing, conflicts, intersections }: PeerDependencyIssues): string[] {
  const sections: string[] = []
  for (const [peerName, issues] of Object.entries(bad)) {
    const header = `${chalk.yellowBright('✕ unmet peer')} ${chalk.bold(peerName)}`
    for (const [foundVersion, group] of groupByFoundVersion(issues)) {
      const installed = `  ${chalk.cyan('Installed:')} ${chalk.dim(foundVersion)}`
      sections.push(`${header}\n${installed}\n${formatRequiredBy(group)}`)
    }
  }

  for (const [peerName, issues] of Object.entries(missing)) {
    if (!intersections[peerName] && !conflicts.includes(peerName)) continue
    const conflict = conflicts.includes(peerName)
    const header = conflict
      ? `${chalk.red('✕ conflicting peer')} ${chalk.bold(peerName)}`
      : `${chalk.red('✕ missing peer')} ${chalk.bold(peerName)}`
    sections.push(`${header}\n${formatRequiredBy(issues)}`)
  }
  return sections
}

function indent (section: string): string {
  return section.split('\n').map((line) => `  ${line}`).join('\n')
}

function formatRequiredBy (issues: Array<{ parents: Array<{ name: string, version: string }>, wantedRange: string }>): string {
  const byRange = new Map<string, Set<string>>()
  for (const issue of issues) {
    const declaring = issue.parents[issue.parents.length - 1]
    const pkg = declaring ? `${declaring.name}@${declaring.version}` : '<unknown>'
    if (!byRange.has(issue.wantedRange)) {
      byRange.set(issue.wantedRange, new Set())
    }
    byRange.get(issue.wantedRange)!.add(pkg)
  }
  const lines: string[] = [`  ${chalk.cyan('Wanted:')}`]
  for (const [range, pkgs] of byRange) {
    lines.push(`    ${chalk.cyanBright(formatRange(range))}${chalk.cyan(':')}`)
    for (const pkg of pkgs) {
      lines.push(`      ${chalk.dim(pkg)}`)
    }
  }
  return lines.join('\n')
}

function formatRange (range: string): string {
  if (range.includes(' ') || range === '*') {
    return `"${range}"`
  }
  return range
}

function groupByFoundVersion (issues: BadPeerDependencyIssue[]): Map<string, BadPeerDependencyIssue[]> {
  const groups = new Map<string, BadPeerDependencyIssue[]>()
  for (const issue of issues) {
    const list = groups.get(issue.foundVersion)
    if (list) {
      list.push(issue)
    } else {
      groups.set(issue.foundVersion, [issue])
    }
  }
  return groups
}
