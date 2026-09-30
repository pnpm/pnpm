import path from 'node:path'

import { createMatcher } from '@pnpm/config.matcher'
import { parseOverrides } from '@pnpm/config.parse-overrides'
import { parse as parseDependencyPath, refToRelative } from '@pnpm/deps.path'
import { getPeerVersionRange } from '@pnpm/deps.peer-range'
import { PnpmError } from '@pnpm/error'
import {
  getLockfileImporterId,
  type LockfileObject,
  type PackageSnapshot,
  type PackageSnapshots,
  readCurrentLockfile,
  readWantedLockfile,
} from '@pnpm/lockfile.fs'
import { lockfileWalkerGroupImporterSteps, type LockfileWalkerStep } from '@pnpm/lockfile.walker'
import type {
  BadPeerDependencyIssue,
  MissingPeerDependencyIssue,
  ParentPackages,
  PeerDependencyIssues,
  PeerDependencyIssuesByProjects,
  PeerDependencyRules,
  ProjectId,
} from '@pnpm/types'
import semver from 'semver'
import { intersect } from 'semver-range-intersect'

export async function checkPeerDependencies (
  projectPaths: string[],
  opts: {
    lockfileDir: string
    checkWantedLockfileOnly?: boolean
    modulesDir?: string
    peerDependencyRules?: PeerDependencyRules
  }
): Promise<PeerDependencyIssuesByProjects> {
  const modulesDir = opts.modulesDir ?? 'node_modules'
  const lockfile = opts.checkWantedLockfileOnly
    ? await readWantedLockfile(opts.lockfileDir, { ignoreIncompatible: false })
    : await readCurrentLockfile(path.join(opts.lockfileDir, modulesDir, '.pnpm'), { ignoreIncompatible: false })
      ?? await readWantedLockfile(opts.lockfileDir, { ignoreIncompatible: false })
  if (!lockfile) return {}

  const issues = checkPeerDependenciesFromLockfile(projectPaths, lockfile, opts.lockfileDir)
  if (opts.peerDependencyRules) {
    return filterPeerDependencyIssues(issues, opts.peerDependencyRules)
  }
  return issues
}

function checkPeerDependenciesFromLockfile (
  projectPaths: string[],
  lockfile: LockfileObject,
  lockfileDir: string
): PeerDependencyIssuesByProjects {
  const packages = lockfile.packages ?? {}
  const importerIds = projectPaths.map((projectPath) => getLockfileImporterId(lockfileDir, projectPath))
  const walkerSteps = lockfileWalkerGroupImporterSteps(lockfile, importerIds as ProjectId[])
  const result: PeerDependencyIssuesByProjects = {}

  for (const { importerId, step } of walkerSteps) {
    const projectIssues: PeerDependencyIssues = {
      bad: {},
      missing: {},
      conflicts: [],
      intersections: {},
    }

    walkStep(step, packages, [], projectIssues)

    const merged = mergePeers(projectIssues.missing)
    projectIssues.conflicts = merged.conflicts
    projectIssues.intersections = merged.intersections

    result[importerId] = projectIssues
  }

  return result
}

function walkStep (
  step: LockfileWalkerStep,
  packages: PackageSnapshots,
  parents: ParentPackages,
  issues: PeerDependencyIssues
): void {
  for (const { depPath, pkgSnapshot, next } of step.dependencies) {
    const parsed = parseDependencyPath(depPath)
    const pkgName = parsed.name ?? depPath
    const pkgVersion = pkgSnapshot.version ?? parsed.version ?? ''
    const currentParents: ParentPackages = [...parents, { name: pkgName, version: pkgVersion }]

    if (pkgSnapshot.peerDependencies) {
      checkDependencyPeers(pkgSnapshot, currentParents, packages, issues)
    }

    walkStep(next(), packages, currentParents, issues)
  }
}

function checkDependencyPeers (
  pkgSnapshot: PackageSnapshot,
  currentParents: ParentPackages,
  packages: PackageSnapshots,
  issues: PeerDependencyIssues
): void {
  for (const [peerName, rawPeerRange] of Object.entries(pkgSnapshot.peerDependencies ?? {})) {
    const peerRange = getPeerVersionRange(rawPeerRange)
    const isOptional = pkgSnapshot.peerDependenciesMeta?.[peerName]?.optional === true
    const resolvedPeerRef = pkgSnapshot.dependencies?.[peerName] ?? pkgSnapshot.optionalDependencies?.[peerName]

    checkSinglePeer({
      peerName,
      peerRange,
      isOptional,
      resolvedPeerRef,
      currentParents,
      packages,
      issues,
    })
  }
}

interface PeerCheckContext {
  peerName: string
  peerRange: string
  isOptional: boolean
  resolvedPeerRef: string | undefined
  currentParents: ParentPackages
  packages: PackageSnapshots
  issues: PeerDependencyIssues
}

function checkSinglePeer (ctx: PeerCheckContext): void {
  const { peerName, peerRange, isOptional, resolvedPeerRef, currentParents, packages, issues } = ctx
  if (!resolvedPeerRef) {
    if (!isOptional) {
      if (!issues.missing[peerName]) issues.missing[peerName] = []
      issues.missing[peerName].push({
        parents: currentParents,
        optional: isOptional,
        wantedRange: peerRange,
      })
    }
    return
  }

  const peerVersion = extractVersion(resolvedPeerRef, peerName, packages)
  if (peerVersion && !satisfies(peerVersion, peerRange)) {
    if (!issues.bad[peerName]) issues.bad[peerName] = []
    issues.bad[peerName].push({
      parents: currentParents,
      optional: isOptional,
      wantedRange: peerRange,
      foundVersion: peerVersion,
      resolvedFrom: [],
    })
  }
}

function extractVersion (ref: string, alias: string, packages: PackageSnapshots): string | undefined {
  const depPath = refToRelative(ref, alias)
  if (depPath && packages[depPath]) {
    const parsed = parseDependencyPath(depPath)
    return packages[depPath].version ?? parsed.version
  }
  const parsed = parseDependencyPath(`${alias}@${ref}`)
  return parsed.version
}

function satisfies (version: string, range: string): boolean {
  if (range === '*') return true
  return semver.satisfies(version, range, { includePrerelease: true, loose: true })
}

interface AllowedVersions {
  matchAll: Record<string, string[]>
  byParent: Record<string, Record<string, string[]>>
}

interface IssueFilters {
  ignoreMissingMatcher: (name: string) => boolean
  allowAnyMatcher: (name: string) => boolean
  allowedVersions: AllowedVersions
}

function filterPeerDependencyIssues (
  peerDependencyIssuesByProjects: PeerDependencyIssuesByProjects,
  rules: PeerDependencyRules
): PeerDependencyIssuesByProjects {
  const filters: IssueFilters = {
    ignoreMissingMatcher: createMatcher([...new Set(rules.ignoreMissing ?? [])]),
    allowAnyMatcher: createMatcher([...new Set(rules.allowAny ?? [])]),
    allowedVersions: parseAllowedVersions(rules.allowedVersions ?? {}),
  }

  const result: PeerDependencyIssuesByProjects = {}
  for (const [projectId, projectIssues] of Object.entries(peerDependencyIssuesByProjects)) {
    result[projectId] = filterProjectIssues(projectIssues, filters)
  }
  return result
}

function filterProjectIssues (
  issues: PeerDependencyIssues,
  filters: IssueFilters
): PeerDependencyIssues {
  const { missing, intersections } = filterMissingIssues(
    issues.missing,
    issues.intersections,
    filters.ignoreMissingMatcher
  )
  const bad = filterBadIssues(issues.bad, filters)
  return {
    bad,
    missing,
    conflicts: issues.conflicts.filter((peerName) => missing[peerName] != null),
    intersections,
  }
}

function filterMissingIssues (
  missing: Record<string, MissingPeerDependencyIssue[]>,
  intersections: Record<string, string>,
  ignoreMissingMatcher: (name: string) => boolean
): { missing: Record<string, MissingPeerDependencyIssue[]>, intersections: Record<string, string> } {
  const filteredMissing: Record<string, MissingPeerDependencyIssue[]> = {}
  const filteredIntersections: Record<string, string> = {}
  for (const [peerName, issues] of Object.entries(missing)) {
    if (ignoreMissingMatcher(peerName) || issues.every(({ optional }) => optional)) continue
    filteredMissing[peerName] = issues
    if (intersections[peerName]) {
      filteredIntersections[peerName] = intersections[peerName]
    }
  }
  return { missing: filteredMissing, intersections: filteredIntersections }
}

function filterBadIssues (
  bad: Record<string, BadPeerDependencyIssue[]>,
  filters: IssueFilters
): Record<string, BadPeerDependencyIssue[]> {
  const filteredBad: Record<string, BadPeerDependencyIssue[]> = {}
  for (const [peerName, issues] of Object.entries(bad)) {
    if (filters.allowAnyMatcher(peerName)) continue
    const remaining = issues.filter(issue => !isAllowedBadIssue(issue, peerName, filters.allowedVersions))
    if (remaining.length > 0) {
      filteredBad[peerName] = remaining
    }
  }
  return filteredBad
}

function isAllowedBadIssue (
  issue: BadPeerDependencyIssue,
  peerName: string,
  allowed: AllowedVersions
): boolean {
  if (allowed.matchAll[peerName]?.some((range) => semver.satisfies(issue.foundVersion, range))) {
    return true
  }
  const declaringParent = issue.parents.at(-1)
  if (declaringParent && allowed.byParent[declaringParent.name]?.[peerName]?.some((range) => semver.satisfies(issue.foundVersion, range))) {
    return true
  }
  return false
}

function parseAllowedVersions (allowedVersions: Record<string, string>): {
  matchAll: Record<string, string[]>
  byParent: Record<string, Record<string, string[]>>
} {
  let overrides
  try {
    overrides = parseOverrides(allowedVersions)
  } catch (err) {
    throw new PnpmError('INVALID_ALLOWED_VERSION_SELECTOR',
      `${(err as PnpmError).message} in pnpm.peerDependencyRules.allowedVersions`)
  }
  const matchAll: Record<string, string[]> = {}
  const byParent: Record<string, Record<string, string[]>> = {}
  for (const { parentPkg, targetPkg, newBareSpecifier } of overrides) {
    const ranges = newBareSpecifier.split('||').map((v) => v.trim())
    if (parentPkg) {
      if (!byParent[parentPkg.name]) byParent[parentPkg.name] = {}
      byParent[parentPkg.name][targetPkg.name] = ranges
    } else {
      matchAll[targetPkg.name] = ranges
    }
  }
  return { matchAll, byParent }
}

function mergePeers (missingPeers: Record<string, MissingPeerDependencyIssue[]>): {
  conflicts: string[]
  intersections: Record<string, string>
} {
  const conflicts: string[] = []
  const intersections: Record<string, string> = {}
  for (const [peerName, issues] of Object.entries(missingPeers)) {
    if (issues.every(({ optional }) => optional)) continue
    if (issues.length === 1) {
      intersections[peerName] = issues[0].wantedRange
      continue
    }
    const ranges = [...new Set(issues.map(({ wantedRange }) => wantedRange))]
    if (ranges.length === 1) {
      intersections[peerName] = ranges[0]
      continue
    }
    const intersection = safeIntersect(ranges)
    if (intersection === null) {
      conflicts.push(peerName)
    } else {
      intersections[peerName] = intersection
    }
  }
  return { conflicts, intersections }
}

function safeIntersect (ranges: string[]): string | null {
  try {
    return intersect(...ranges)
  } catch {
    return null
  }
}
