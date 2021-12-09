import { PeerDependencyIssues } from '@pnpm/resolve-dependencies'
import archy from 'archy'
import chalk from 'chalk'

export default function (peerDependencyIssues: PeerDependencyIssues) {
  const projects = {} as Record<string, PkgNode>
  for (const [peerName, issues] of Object.entries(peerDependencyIssues.missing)) {
    for (const issue of issues) {
      const projectPath = issue.location.projectPath || '<ROOT>'
      if (!projects[projectPath]) {
        projects[projectPath] = { dependencies: {}, wantedPeers: [] }
      }
      createTree(projects[projectPath], issue.location.parents, `${chalk.red('✕ missing peer')} ${peerName}@"${issue.peerRange}"`)
    }
  }
  for (const [peerName, issues] of Object.entries(peerDependencyIssues.bad)) {
    for (const issue of issues) {
      const projectPath = issue.location.projectPath || '<ROOT>'
      if (!projects[projectPath]) {
        projects[projectPath] = { dependencies: {}, wantedPeers: [] }
      }
      // eslint-disable-next-line
      createTree(projects[projectPath], issue.location.parents, `${chalk.red('✕ unmet peer')} ${peerName}@"${issue.peerRange}": found ${issue.foundPeerVersion}`)
    }
  }
  return Object.entries(projects)
    .sort(([projectKey1], [projectKey2]) => projectKey1.localeCompare(projectKey2))
    .map(([projectKey, project]) => archy(toArchyData(projectKey, project))).join('')
}

interface PkgNode {
  wantedPeers: string[]
  dependencies: Record<string, PkgNode>
}

function createTree (pkgNode: PkgNode, pkgs: Array<{ name: string, version: string }>, wantedPeer: string) {
  const [pkg, ...rest] = pkgs
  if (!pkgNode.dependencies[pkg.name]) {
    pkgNode.dependencies[pkg.name] = { dependencies: {}, wantedPeers: [] }
  }
  if (rest.length === 0) {
    pkgNode.dependencies[pkg.name].wantedPeers.push(wantedPeer)
    return
  }
  createTree(pkgNode.dependencies[pkg.name], rest, wantedPeer)
}

function toArchyData (depName: string, pkgNode: PkgNode): archy.Data {
  const result: archy.Data = {
    label: depName,
    nodes: [],
  }
  for (const wantedPeer of pkgNode.wantedPeers) {
    result.nodes!.push(wantedPeer)
  }
  for (const [depName, node] of Object.entries(pkgNode.dependencies)) {
    result.nodes!.push(toArchyData(depName, node))
  }
  return result
}
