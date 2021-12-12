import { PeerDependencyIssues } from '@pnpm/types'
import archy from 'archy'
import chalk from 'chalk'

export default function (
  {
    bad,
    missing,
    missingMergedByProjects,
  }: PeerDependencyIssues
) {
  const projects = {} as Record<string, PkgNode>
  for (const [peerName, issues] of Object.entries(missing)) {
    for (const issue of issues) {
      const projectId = issue.location.projectId
      if (!projects[projectId]) {
        projects[projectId] = { dependencies: {}, peerIssues: [] }
      }
      createTree(projects[projectId], issue.location.parents, `${chalk.red('✕ missing peer')} ${peerName}@"${issue.wantedRange}"`)
    }
  }
  for (const [peerName, issues] of Object.entries(bad)) {
    for (const issue of issues) {
      const projectId = issue.location.projectId
      if (!projects[projectId]) {
        projects[projectId] = { dependencies: {}, peerIssues: [] }
      }
      // eslint-disable-next-line
      createTree(projects[projectId], issue.location.parents, `${chalk.red('✕ unmet peer')} ${peerName}@"${issue.wantedRange}": found ${issue.foundVersion}`)
    }
  }
  return Object.entries(projects)
    .sort(([projectKey1], [projectKey2]) => projectKey1.localeCompare(projectKey2))
    .map(([projectKey, project]) => {
      let label = projectKey
      for (const conflict of missingMergedByProjects[projectKey].conflicts) {
        label += `\n${chalk.red(`✕ conflicting ranges for ${conflict}`)}`
      }
      for (const { peerName, versionRange } of missingMergedByProjects[projectKey].intersections) {
        label += `\nadd ${peerName}@"${versionRange}"`
      }
      return archy(toArchyData(label, project))
    }).join('')
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
