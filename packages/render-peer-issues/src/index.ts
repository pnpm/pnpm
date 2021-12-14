import { PeerDependencyIssuesByProjects } from '@pnpm/types'
import archy from 'archy'
import chalk from 'chalk'
import cliColumns from 'cli-columns'

export default function (
  peerDependencyIssuesByProjects: PeerDependencyIssuesByProjects,
  opts?: { width?: number }
) {
  const projects = {} as Record<string, PkgNode>
  for (const [projectId, { bad, missing, conflicts, intersections }] of Object.entries(peerDependencyIssuesByProjects)) {
    projects[projectId] = { dependencies: {}, peerIssues: [] }
    for (const [peerName, issues] of Object.entries(missing)) {
      if (
        !conflicts.includes(peerName) &&
        intersections[peerName] == null
      ) {
        continue
      }
      for (const issue of issues) {
        createTree(projects[projectId], issue.parents, `${chalk.red('✕ missing peer')} ${formatNameAndRange(peerName, issue.wantedRange)}`)
      }
    }
    for (const [peerName, issues] of Object.entries(bad)) {
      for (const { parents, foundVersion, wantedRange } of issues) {
        // eslint-disable-next-line
        createTree(projects[projectId], parents, `${chalk.red('✕ unmet peer')} ${formatNameAndRange(peerName, wantedRange)}: found ${foundVersion}`)
      }
    }
  }
  const cliColumnsOptions = {
    newline: '\n  ',
    width: (opts?.width ?? process.stdout.columns) - 2,
  }
  return Object.entries(projects)
    .filter(([, project]) => Object.keys(project.dependencies).length > 0)
    .sort(([projectKey1], [projectKey2]) => projectKey1.localeCompare(projectKey2))
    .map(([projectKey, project]) => {
      const summaries = []
      const { conflicts, intersections } = peerDependencyIssuesByProjects[projectKey]
      if (conflicts.length) {
        summaries.push(
          chalk.red(`✕ Conflicting peer dependencies:\n  ${cliColumns(conflicts, cliColumnsOptions)}`)
        )
      }
      if (Object.keys(intersections).length) {
        summaries.push(
          `Peer dependencies that should be installed:\n  ${cliColumns(Object.entries(intersections).map(([name, version]) => formatNameAndRange(name, version)), cliColumnsOptions)}`
        )
      }
      const title = chalk.white(projectKey)
      return `${archy(toArchyData(title, project))}${summaries.join('\n')}`
    }).join('\n\n')
}

function formatNameAndRange (name: string, range: string) {
  if (range.includes(' ')) {
    return `${name}@"${range}"`
  }
  return `${name}@${range}`
}

interface PkgNode {
  peerIssues: string[]
  dependencies: Record<string, PkgNode>
}

function createTree (pkgNode: PkgNode, pkgs: Array<{ name: string, version: string }>, issueText: string) {
  const [pkg, ...rest] = pkgs
  if (!pkgNode.dependencies[pkg.name]) {
    pkgNode.dependencies[pkg.name] = { dependencies: {}, peerIssues: [] }
  }
  if (rest.length === 0) {
    pkgNode.dependencies[pkg.name].peerIssues.push(issueText)
    return
  }
  createTree(pkgNode.dependencies[pkg.name], rest, issueText)
}

function toArchyData (depName: string, pkgNode: PkgNode): archy.Data {
  const result: Required<archy.Data> = {
    label: depName,
    nodes: [],
  }
  for (const wantedPeer of pkgNode.peerIssues) {
    result.nodes.push(wantedPeer)
  }
  for (const [depName, node] of Object.entries(pkgNode.dependencies)) {
    result.nodes.push(toArchyData(depName, node))
  }
  return result
}
