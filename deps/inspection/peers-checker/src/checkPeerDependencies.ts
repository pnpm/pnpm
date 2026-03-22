import path from 'node:path'

import { createMatcher } from '@pnpm/config.matcher'
import { parseOverrides } from '@pnpm/config.parse-overrides'
import { parse as parseDependencyPath, refToRelative } from '@pnpm/deps.path'
import {
  getLockfileImporterId,
  type LockfileObject,
  type PackageSnapshots,
  readCurrentLockfile,
  readWantedLockfile,
} from '@pnpm/lockfile.fs'
import type {
  BadPeerDependencyIssue,
  DepPath,
  MissingPeerDependencyIssue,
  ParentPackages,
  PeerDependencyIssues,
  PeerDependencyIssuesByProjects,
  PeerDependencyRules,
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
  const result: PeerDependencyIssuesByProjects = {}

  for (const projectPath of projectPaths) {
    const importerId = getLockfileImporterId(lockfileDir, projectPath)
    const importer = lockfile.importers[importerId]
    if (!importer) continue

    const projectIssues: PeerDependencyIssues = {
      bad: {},
      missing: {},
      conflicts: [],
      intersections: {},
    }

    const allDeps: Record<string, string> = {
      ...importer.dependencies,
      ...importer.devDependencies,
      ...importer.optionalDependencies,
    }

    const visited = new Set<DepPath>()

    for (const [alias, version] of Object.entries(allDeps)) {
      const depPath = refToRelative(version, alias)
      if (!depPath) continue
      walkDependency(depPath, alias, packages, [], visited, projectIssues)
    }

    const merged = mergePeers(projectIssues.missing)
    projectIssues.conflicts = merged.conflicts
    projectIssues.intersections = merged.intersections

    result[importerId] = projectIssues
  }

  return result
}

function walkDependency (
  depPath: DepPath,
  alias: string,
  packages: PackageSnapshots,
  parents: ParentPackages,
  visited: Set<DepPath>,
  issues: PeerDependencyIssues
): void {
  if (visited.has(depPath)) return
  visited.add(depPath)

  const snapshot = packages[depPath]
  if (!snapshot) return

  const parsed = parseDependencyPath(depPath)
  const pkgName = parsed.name ?? alias
  const pkgVersion = snapshot.version ?? parsed.version ?? ''
  const currentParents: ParentPackages = [...parents, { name: pkgName, version: pkgVersion }]

  if (snapshot.peerDependencies) {
    for (const [peerName, peerRange] of Object.entries(snapshot.peerDependencies)) {
      const isOptional = snapshot.peerDependenciesMeta?.[peerName]?.optional === true
      const resolvedPeerRef = snapshot.dependencies?.[peerName] ?? snapshot.optionalDependencies?.[peerName]

      if (!resolvedPeerRef) {
        if (!isOptional) {
          if (!issues.missing[peerName]) issues.missing[peerName] = []
          issues.missing[peerName].push({
            parents: currentParents,
            optional: isOptional,
            wantedRange: peerRange,
          })
        }
      } else {
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
    }
  }

  const allDeps: Record<string, string> = {
    ...snapshot.dependencies,
    ...snapshot.optionalDependencies,
  }

  for (const [childAlias, childVersion] of Object.entries(allDeps)) {
    const childDepPath = refToRelative(childVersion, childAlias)
    if (!childDepPath) continue
    walkDependency(childDepPath, childAlias, packages, currentParents, visited, issues)
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
  return semver.satisfies(version, range, { includePrerelease: true })
}

function filterPeerDependencyIssues (
  peerDependencyIssuesByProjects: PeerDependencyIssuesByProjects,
  rules: PeerDependencyRules
): PeerDependencyIssuesByProjects {
  const ignoreMissingMatcher = createMatcher([...new Set(rules.ignoreMissing ?? [])])
  const allowAnyMatcher = createMatcher([...new Set(rules.allowAny ?? [])])
  const { matchAll: allowedVersionsMatchAll, byParent: allowedVersionsByParent } = parseAllowedVersions(rules.allowedVersions ?? {})

  const result: PeerDependencyIssuesByProjects = {}

  for (const [projectId, { bad, missing, conflicts, intersections }] of Object.entries(peerDependencyIssuesByProjects)) {
    const filteredMissing: Record<string, MissingPeerDependencyIssue[]> = {}
    const filteredBad: Record<string, BadPeerDependencyIssue[]> = {}
    const filteredIntersections: Record<string, string> = {}

    for (const [peerName, issues] of Object.entries(missing)) {
      if (ignoreMissingMatcher(peerName) || issues.every(({ optional }) => optional)) continue
      filteredMissing[peerName] = issues
      if (intersections[peerName]) {
        filteredIntersections[peerName] = intersections[peerName]
      }
    }

    for (const [peerName, issues] of Object.entries(bad)) {
      if (allowAnyMatcher(peerName)) continue
      const remaining = issues.filter(
        (issue) => {
          if (allowedVersionsMatchAll[peerName]?.some(
            (range) => semver.satisfies(issue.foundVersion, range)
          )) return false
          for (const parent of issue.parents) {
            if (allowedVersionsByParent[parent.name]?.[peerName]?.some(
              (range) => semver.satisfies(issue.foundVersion, range)
            )) return false
          }
          return true
        }
      )
      if (remaining.length > 0) {
        filteredBad[peerName] = remaining
      }
    }

    result[projectId] = {
      bad: filteredBad,
      missing: filteredMissing,
      conflicts,
      intersections: filteredIntersections,
    }
  }

  return result
}

function parseAllowedVersions (allowedVersions: Record<string, string>): {
  matchAll: Record<string, string[]>
  byParent: Record<string, Record<string, string[]>>
} {
  const overrides = parseOverrides(allowedVersions)
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
