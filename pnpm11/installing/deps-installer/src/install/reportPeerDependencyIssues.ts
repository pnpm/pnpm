import { createMatcher } from '@pnpm/config.matcher'
import { parseOverrides, type VersionOverride } from '@pnpm/config.parse-overrides'
import { peerDependencyIssuesLogger } from '@pnpm/core-loggers'
import { PnpmError } from '@pnpm/error'
import type { BadPeerDependencyIssue, PeerDependencyIssues, PeerDependencyIssuesByProjects, PeerDependencyRules } from '@pnpm/types'
import { isEmpty } from 'ramda'
import semver from 'semver'

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
  const filters = createPeerIssueFilters(rules)
  const newPeerDependencyIssuesByProjects: PeerDependencyIssuesByProjects = {}
  for (const [projectId, peerIssuesOfProject] of Object.entries(peerDependencyIssuesByProjects)) {
    newPeerDependencyIssuesByProjects[projectId] = filterProjectPeerIssues(peerIssuesOfProject, filters)
  }
  return newPeerDependencyIssuesByProjects
}

interface PeerIssueFilters extends ParsedAllowedVersions {
  ignoreMissingMatcher: (peerName: string) => boolean
  allowAnyMatcher: (peerName: string) => boolean
}

function createPeerIssueFilters (rules: PeerDependencyRules): PeerIssueFilters {
  const ignoreMissingPatterns = [...new Set(rules?.ignoreMissing ?? [])]
  const ignoreMissingMatcher = createMatcher(ignoreMissingPatterns)
  const allowAnyPatterns = [...new Set(rules?.allowAny ?? [])]
  const allowAnyMatcher = createMatcher(allowAnyPatterns)
  return {
    ignoreMissingMatcher,
    allowAnyMatcher,
    ...parseAllowedVersions(rules?.allowedVersions ?? {}),
  }
}

function filterProjectPeerIssues (
  { bad, missing, conflicts, intersections }: PeerDependencyIssues,
  filters: PeerIssueFilters
): PeerDependencyIssues {
  const filteredMissing: PeerDependencyIssues['missing'] = {}
  const filteredIntersections: PeerDependencyIssues['intersections'] = {}
  for (const [peerName, issues] of Object.entries(missing)) {
    if (
      filters.ignoreMissingMatcher(peerName) || issues.every(({ optional }) => optional)
    ) {
      continue
    }
    filteredMissing[peerName] = issues
    if (Object.hasOwn(intersections, peerName)) {
      filteredIntersections[peerName] = intersections[peerName]
    }
  }
  return {
    bad: filterBadPeers(bad, filters),
    missing: filteredMissing,
    conflicts: conflicts.filter((peerName) => Object.hasOwn(filteredMissing, peerName)),
    intersections: filteredIntersections,
  }
}

function filterBadPeers (
  bad: PeerDependencyIssues['bad'],
  filters: PeerIssueFilters
): PeerDependencyIssues['bad'] {
  const filteredBad: PeerDependencyIssues['bad'] = {}
  for (const [peerName, issues] of Object.entries(bad)) {
    if (filters.allowAnyMatcher(peerName)) continue
    const filteredIssues = issues.filter((issue) => !isAllowedBadPeer(peerName, issue, filters))
    if (filteredIssues.length) {
      filteredBad[peerName] = filteredIssues
    }
  }
  return filteredBad
}

function isAllowedBadPeer (
  peerName: string,
  issue: BadPeerDependencyIssue,
  { allowedVersionsMatchAll, allowedVersionsByParentPkgName }: ParsedAllowedVersions
): boolean {
  if (allowedVersionsMatchAll[peerName]?.some((range) => semver.satisfies(issue.foundVersion, range))) return true
  const currentParentPkg = issue.parents.at(-1)
  if (!currentParentPkg) return false
  const parentRules = allowedVersionsByParentPkgName[peerName]?.[currentParentPkg.name]
  if (!parentRules) return false
  const allowedVersionsByParent = allowedRangesForParent(parentRules, currentParentPkg)
  return allowedVersionsByParent[peerName]?.some((range) => semver.satisfies(issue.foundVersion, range)) === true
}

function allowedRangesForParent (
  parentRules: AllowedVersionsByParentPkgName[string][string],
  currentParentPkg: { version: string }
): Record<string, string[]> {
  const allowedVersionsByParent: Record<string, string[]> = {}
  for (const { targetPkg, parentPkg, ranges } of parentRules) {
    if (parentRangeAdmits(parentPkg.bareSpecifier, currentParentPkg.version)) {
      allowedVersionsByParent[targetPkg.name] = ranges
    }
  }
  return allowedVersionsByParent
}

function parentRangeAdmits (parentRange: string | undefined, parentVersion: string): boolean {
  if (!parentRange) return true
  return Boolean(parentVersion) &&
    (isSubRange(parentRange, parentVersion) || semver.satisfies(parentVersion, parentRange))
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
  // Null-prototype maps, so a peer or parent named `constructor` reads nothing
  // off `Object.prototype`.
  const allowedVersionsMatchAll: Record<string, string[]> = Object.create(null)
  const allowedVersionsByParentPkgName: AllowedVersionsByParentPkgName = Object.create(null)
  for (const { parentPkg, targetPkg, newBareSpecifier } of overrides) {
    const ranges = parseVersions(newBareSpecifier)
    if (!parentPkg) {
      allowedVersionsMatchAll[targetPkg.name] = ranges
      continue
    }
    if (!allowedVersionsByParentPkgName[targetPkg.name]) {
      allowedVersionsByParentPkgName[targetPkg.name] = Object.create(null)
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
