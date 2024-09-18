import { PnpmError } from '@pnpm/error'
import { createMatcher } from '@pnpm/matcher'
import {
  type BadPeerDependencyIssue,
  type PeerDependencyIssuesByProjects,
  type PeerDependencyRules,
} from '@pnpm/types'
import { parseOverrides, type VersionOverride } from '@pnpm/parse-overrides'
import archy from 'archy'
import chalk from 'chalk'
import cliColumns from 'cli-columns'
import semver from 'semver'

export function renderPeerIssues (
  peerDependencyIssuesByProjects: PeerDependencyIssuesByProjects,
  opts?: {
    rules?: PeerDependencyRules
    width?: number
  }
): string {
  const ignoreMissingPatterns = [...new Set(opts?.rules?.ignoreMissing ?? [])]
  const ignoreMissingMatcher = createMatcher(ignoreMissingPatterns)
  const allowAnyPatterns = [...new Set(opts?.rules?.allowAny ?? [])]
  const allowAnyMatcher = createMatcher(allowAnyPatterns)
  const { allowedVersionsMatchAll, allowedVersionsByParentPkgName } = parseAllowedVersions(opts?.rules?.allowedVersions ?? {})
  const projects = {} as Record<string, PkgNode>
  for (const [projectId, { bad, missing, conflicts, intersections }] of Object.entries(peerDependencyIssuesByProjects)) {
    projects[projectId] = { dependencies: {}, peerIssues: [] }
    for (const [peerName, issues] of Object.entries(missing)) {
      if (
        !conflicts.includes(peerName) &&
        intersections[peerName] == null ||
        ignoreMissingMatcher(peerName)
      ) {
        continue
      }
      for (const issue of issues) {
        createTree(projects[projectId], issue.parents, `${chalk.red('✕ missing peer')} ${formatNameAndRange(peerName, issue.wantedRange)}`)
      }
    }
    for (const [peerName, issues] of Object.entries(bad)) {
      if (allowAnyMatcher(peerName)) continue
      for (const issue of issues) {
        if (allowedVersionsMatchAll[peerName]?.some((range) => semver.satisfies(issue.foundVersion, range))) continue
        const currentParentPkg = issue.parents.at(-1)
        if (currentParentPkg && allowedVersionsByParentPkgName[peerName]?.[currentParentPkg.name]) {
          const allowedVersionsByParent: Record<string, string[]> = {}
          for (const { targetPkg, parentPkg, ranges } of allowedVersionsByParentPkgName[peerName][currentParentPkg.name]) {
            if (!parentPkg.pref || currentParentPkg.version &&
              (isSubRange(parentPkg.pref, currentParentPkg.version) || semver.satisfies(currentParentPkg.version, parentPkg.pref))) {
              allowedVersionsByParent[targetPkg.name] = ranges
            }
          }
          if (allowedVersionsByParent[peerName]?.some((range) => semver.satisfies(issue.foundVersion, range))) continue
        }
        createTree(projects[projectId], issue.parents, formatUnmetPeerMessage({
          peerName,
          ...issue,
        }))
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
      let summariesConcatenated = summaries.join('\n')
      if (summariesConcatenated) {
        summariesConcatenated += '\n'
      }
      return `${archy(toArchyData(title, project))}${summariesConcatenated}`
    }).join('\n')
}

function formatUnmetPeerMessage (
  { foundVersion, peerName, wantedRange, resolvedFrom }: BadPeerDependencyIssue & {
    peerName: string
  }
) {
  const nameAndRange = formatNameAndRange(peerName, wantedRange)
  if (resolvedFrom && resolvedFrom.length > 0) {
    return `✕ unmet peer ${nameAndRange}: found ${foundVersion} in ${resolvedFrom[resolvedFrom.length - 1].name}`
  }
  return `${chalk.yellowBright('✕ unmet peer')} ${nameAndRange}: found ${foundVersion}`
}

function formatNameAndRange (name: string, range: string) {
  if (range.includes(' ') || range === '*') {
    return `${name}@"${range}"`
  }
  return `${name}@${range}`
}

interface PkgNode {
  peerIssues: string[]
  dependencies: Record<string, PkgNode>
}

function createTree (pkgNode: PkgNode, pkgs: Array<{ name: string, version: string }>, issueText: string): void {
  const [pkg, ...rest] = pkgs
  const label = `${pkg.name} ${chalk.grey(pkg.version)}`
  if (!pkgNode.dependencies[label]) {
    pkgNode.dependencies[label] = { dependencies: {}, peerIssues: [] }
  }
  if (rest.length === 0) {
    pkgNode.dependencies[label].peerIssues.push(issueText)
    return
  }
  createTree(pkgNode.dependencies[label], rest, issueText)
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

type AllowedVersionsByParentPkgName = Record<string, Record<string, Array<Required<Pick<VersionOverride, 'parentPkg' | 'targetPkg'>> & { ranges: string[] }>>>

interface ParsedAllowedVersions {
  allowedVersionsMatchAll: Record<string, string[]>
  allowedVersionsByParentPkgName: AllowedVersionsByParentPkgName
}

function parseAllowedVersions (allowedVersions: Record<string, string>): ParsedAllowedVersions {
  const overrides = tryParseAllowedVersions(allowedVersions)
  const allowedVersionsMatchAll: Record<string, string[]> = {}
  const allowedVersionsByParentPkgName: AllowedVersionsByParentPkgName = {}
  for (const { parentPkg, targetPkg, newPref } of overrides) {
    const ranges = parseVersions(newPref)
    if (!parentPkg) {
      allowedVersionsMatchAll[targetPkg.name] = ranges
      continue
    }
    if (!allowedVersionsByParentPkgName[targetPkg.name]) {
      allowedVersionsByParentPkgName[targetPkg.name] = {}
    }
    if (!allowedVersionsByParentPkgName[targetPkg.name][parentPkg.name]) {
      allowedVersionsByParentPkgName[targetPkg.name][parentPkg.name] = []
    }
    allowedVersionsByParentPkgName[targetPkg.name][parentPkg.name].push({
      parentPkg,
      targetPkg,
      ranges,
    })
  }
  return {
    allowedVersionsMatchAll,
    allowedVersionsByParentPkgName,
  }
}

function tryParseAllowedVersions (allowedVersions: Record<string, string>): VersionOverride[] {
  try {
    return parseOverrides(allowedVersions ?? {})
  } catch (err) {
    throw new PnpmError('INVALID_ALLOWED_VERSION_SELECTOR',
      `${(err as PnpmError).message} in pnpm.peerDependencyRules.allowedVersions`)
  }
}

function parseVersions (versions: string): string[] {
  return versions.split('||').map(v => v.trim())
}

function isSubRange (superRange: string | undefined, subRange: string): boolean {
  return !superRange ||
  subRange === superRange ||
  semver.validRange(subRange) != null &&
  semver.validRange(superRange) != null &&
  semver.subset(subRange, superRange)
}
