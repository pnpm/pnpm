import { PnpmError } from '@pnpm/error'
import { createMatcher } from '@pnpm/matcher'
import { peerDependencyIssuesLogger } from '@pnpm/core-loggers'
import { type PeerDependencyIssuesByProjects, type PeerDependencyRules, type BadPeerDependencyIssue } from '@pnpm/types'
import semver from 'semver'
import { isEmpty } from 'ramda'
import { parseOverrides, type VersionOverride } from '@pnpm/parse-overrides'

export function reportPeerDependencyIssues (
  peerDependencyIssuesByProjects: PeerDependencyIssuesByProjects,
  opts: {
    lockfileDir: string
    rules?: PeerDependencyRules
    strictPeerDependencies: boolean
  }
): void {
  const newPeerDependencyIssuesByProjects = filterPeerDependencyIssues(peerDependencyIssuesByProjects, opts.rules)
  if (
    Object.values(newPeerDependencyIssuesByProjects).every((peerIssuesOfProject) =>
      isEmpty(peerIssuesOfProject.bad) && (
        isEmpty(peerIssuesOfProject.missing) ||
        peerIssuesOfProject.conflicts.length === 0 && Object.keys(peerIssuesOfProject.intersections).length === 0
      ))
  ) return
  if (opts.strictPeerDependencies) {
    throw new PeerDependencyIssuesError(newPeerDependencyIssuesByProjects)
  }
  peerDependencyIssuesLogger.debug({
    issuesByProjects: newPeerDependencyIssuesByProjects,
  })
}

export function filterPeerDependencyIssues (
  peerDependencyIssuesByProjects: PeerDependencyIssuesByProjects,
  rules?: PeerDependencyRules
): PeerDependencyIssuesByProjects {
  if (!rules) return peerDependencyIssuesByProjects
  const ignoreMissingPatterns = [...new Set(rules?.ignoreMissing ?? [])]
  const ignoreMissingMatcher = createMatcher(ignoreMissingPatterns)
  const allowAnyPatterns = [...new Set(rules?.allowAny ?? [])]
  const allowAnyMatcher = createMatcher(allowAnyPatterns)
  const { allowedVersionsMatchAll, allowedVersionsByParentPkgName } = parseAllowedVersions(rules?.allowedVersions ?? {})
  const newPeerDependencyIssuesByProjects: PeerDependencyIssuesByProjects = {}
  for (const [projectId, { bad, missing, conflicts, intersections }] of Object.entries(peerDependencyIssuesByProjects)) {
    newPeerDependencyIssuesByProjects[projectId] = { bad: {}, missing: {}, conflicts, intersections }
    for (const [peerName, issues] of Object.entries(missing)) {
      if (
        ignoreMissingMatcher(peerName) || issues.every(({ optional }) => optional)
      ) {
        continue
      }
      newPeerDependencyIssuesByProjects[projectId].missing[peerName] = issues
    }
    for (const [peerName, issues] of Object.entries(bad)) {
      if (allowAnyMatcher(peerName)) continue
      const filteredIssues: BadPeerDependencyIssue[] = []
      for (const issue of issues) {
        if (allowedVersionsMatchAll[peerName]?.some((range) => semver.satisfies(issue.foundVersion, range))) continue
        const currentParentPkg = issue.parents.at(-1)
        if (currentParentPkg && allowedVersionsByParentPkgName[peerName]?.[currentParentPkg.name]) {
          const allowedVersionsByParent: Record<string, string[]> = {}
          for (const { targetPkg, parentPkg, ranges } of allowedVersionsByParentPkgName[peerName][currentParentPkg.name]) {
            if (!parentPkg.bareSpecifier || currentParentPkg.version &&
              (isSubRange(parentPkg.bareSpecifier, currentParentPkg.version) || semver.satisfies(currentParentPkg.version, parentPkg.bareSpecifier))) {
              allowedVersionsByParent[targetPkg.name] = ranges
            }
          }
          if (allowedVersionsByParent[peerName]?.some((range) => semver.satisfies(issue.foundVersion, range))) continue
        }
        filteredIssues.push(issue)
      }
      if (filteredIssues.length) {
        newPeerDependencyIssuesByProjects[projectId].bad[peerName] = filteredIssues
      }
    }
  }
  return newPeerDependencyIssuesByProjects
}

function isSubRange (superRange: string | undefined, subRange: string): boolean {
  return !superRange ||
  subRange === superRange ||
  semver.validRange(subRange) != null &&
  semver.validRange(superRange) != null &&
  semver.subset(subRange, superRange)
}

type AllowedVersionsByParentPkgName = Record<string, Record<string, Array<Required<Pick<VersionOverride, 'parentPkg' | 'targetPkg'>> & { ranges: string[] }>>>

interface ParsedAllowedVersions {
  allowedVersionsMatchAll: Record<string, string[]>
  allowedVersionsByParentPkgName: AllowedVersionsByParentPkgName
}

function tryParseAllowedVersions (allowedVersions: Record<string, string>): VersionOverride[] {
  try {
    return parseOverrides(allowedVersions ?? {})
  } catch (err) {
    throw new PnpmError('INVALID_ALLOWED_VERSION_SELECTOR',
      `${(err as PnpmError).message} in pnpm.peerDependencyRules.allowedVersions`)
  }
}

function parseAllowedVersions (allowedVersions: Record<string, string>): ParsedAllowedVersions {
  const overrides = tryParseAllowedVersions(allowedVersions)
  const allowedVersionsMatchAll: Record<string, string[]> = {}
  const allowedVersionsByParentPkgName: AllowedVersionsByParentPkgName = {}
  for (const { parentPkg, targetPkg, newBareSpecifier } of overrides) {
    const ranges = parseVersions(newBareSpecifier)
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

function parseVersions (versions: string): string[] {
  return versions.split('||').map(v => v.trim())
}

export class PeerDependencyIssuesError extends PnpmError {
  issuesByProjects: PeerDependencyIssuesByProjects
  constructor (issues: PeerDependencyIssuesByProjects) {
    super('PEER_DEP_ISSUES', 'Unmet peer dependencies')
    this.issuesByProjects = issues
  }
}
