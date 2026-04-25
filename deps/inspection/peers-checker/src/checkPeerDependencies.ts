import path from 'node:path'

import { createMatcher } from '@pnpm/config.matcher'
import { parseOverrides } from '@pnpm/config.parse-overrides'
import { parse as parseDependencyPath, refToRelative } from '@pnpm/deps.path'
import { PnpmError } from '@pnpm/error'
import {
  getLockfileImporterId,
  type LockfileObject,
  type PackageSnapshots,
  type ProjectSnapshot,
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
    /**
     * When true (default), peer dependencies provided by the workspace root
     * importer are not reported as missing or unmet for non-root importers.
     * This matches the behavior of `resolvePeersFromWorkspaceRoot` during
     * install: in a monorepo, peers can legitimately be satisfied by a
     * singleton installed at the workspace root.
     */
    resolvePeersFromWorkspaceRoot?: boolean
  }
): Promise<PeerDependencyIssuesByProjects> {
  const modulesDir = opts.modulesDir ?? 'node_modules'
  const lockfile = opts.checkWantedLockfileOnly
    ? await readWantedLockfile(opts.lockfileDir, { ignoreIncompatible: false })
    : await readCurrentLockfile(path.join(opts.lockfileDir, modulesDir, '.pnpm'), { ignoreIncompatible: false })
      ?? await readWantedLockfile(opts.lockfileDir, { ignoreIncompatible: false })
  if (!lockfile) return {}

  const issues = checkPeerDependenciesFromLockfile(projectPaths, lockfile, opts.lockfileDir, {
    resolvePeersFromWorkspaceRoot: opts.resolvePeersFromWorkspaceRoot ?? true,
  })
  if (opts.peerDependencyRules) {
    return filterPeerDependencyIssues(issues, opts.peerDependencyRules)
  }
  return issues
}

interface RootPeerProvider {
  /** Pre-resolved version provided by the workspace root for a given peer name. */
  resolvedVersionByPeerName: Map<string, string>
}

function checkPeerDependenciesFromLockfile (
  projectPaths: string[],
  lockfile: LockfileObject,
  lockfileDir: string,
  opts: { resolvePeersFromWorkspaceRoot: boolean }
): PeerDependencyIssuesByProjects {
  const packages = lockfile.packages ?? {}
  const importerIds = projectPaths.map((p) => getLockfileImporterId(lockfileDir, p))
  const walkerSteps = lockfileWalkerGroupImporterSteps(lockfile, importerIds as ProjectId[])
  const result: PeerDependencyIssuesByProjects = {}

  // Build a lookup of peer-eligible packages provided by the workspace root.
  // We only do this when there's an actual workspace (multiple importers) and
  // a root importer exists at id ".".
  const rootPeerProvider = opts.resolvePeersFromWorkspaceRoot
    ? buildRootPeerProvider(lockfile, packages, importerIds)
    : undefined

  for (const { importerId, step } of walkerSteps) {
    const projectIssues: PeerDependencyIssues = {
      bad: {},
      missing: {},
      conflicts: [],
      intersections: {},
    }

    // The workspace root provides peers; sub-projects can rely on those.
    // For the root importer itself, we don't apply this filter because its
    // peers must be satisfied within its own dependency tree.
    const provider = importerId === '.' ? undefined : rootPeerProvider

    walkStep(step, packages, [], projectIssues, provider)

    const merged = mergePeers(projectIssues.missing)
    projectIssues.conflicts = merged.conflicts
    projectIssues.intersections = merged.intersections

    result[importerId] = projectIssues
  }

  return result
}

function buildRootPeerProvider (
  lockfile: LockfileObject,
  packages: PackageSnapshots,
  importerIds: string[]
): RootPeerProvider | undefined {
  // Only meaningful in a workspace where the root and at least one other
  // importer are being checked together.
  if (importerIds.length < 2 || !importerIds.includes('.')) return undefined
  const rootImporter: ProjectSnapshot | undefined = lockfile.importers?.['.' as ProjectId]
  if (!rootImporter) return undefined

  const resolvedVersionByPeerName = new Map<string, string>()
  const directRefs: Record<string, string> = {
    ...(rootImporter.dependencies ?? {}),
    ...(rootImporter.devDependencies ?? {}),
    ...(rootImporter.optionalDependencies ?? {}),
  }
  for (const [alias, ref] of Object.entries(directRefs)) {
    if (typeof ref !== 'string') continue
    // Workspace links and link: refs cannot be inspected via packages map.
    if (ref.startsWith('link:') || ref.startsWith('file:')) continue
    const version = extractVersion(ref, alias, packages)
    if (version) resolvedVersionByPeerName.set(alias, version)
  }
  return resolvedVersionByPeerName.size > 0 ? { resolvedVersionByPeerName } : undefined
}

function walkStep (
  step: LockfileWalkerStep,
  packages: PackageSnapshots,
  parents: ParentPackages,
  issues: PeerDependencyIssues,
  rootPeerProvider?: RootPeerProvider
): void {
  for (const { depPath, pkgSnapshot, next } of step.dependencies) {
    const parsed = parseDependencyPath(depPath)
    const pkgName = parsed.name ?? depPath
    const pkgVersion = pkgSnapshot.version ?? parsed.version ?? ''
    const currentParents: ParentPackages = [...parents, { name: pkgName, version: pkgVersion }]

    if (pkgSnapshot.peerDependencies) {
      for (const [peerName, peerRange] of Object.entries(pkgSnapshot.peerDependencies)) {
        const isOptional = pkgSnapshot.peerDependenciesMeta?.[peerName]?.optional === true
        const resolvedPeerRef = pkgSnapshot.dependencies?.[peerName] ?? pkgSnapshot.optionalDependencies?.[peerName]

        if (!resolvedPeerRef) {
          if (isOptional) continue
          // If the workspace root provides this peer at a satisfying version,
          // suppress the warning entirely. This mirrors install-time behavior
          // where peers hoisted at the workspace root are considered resolved
          // for sub-projects.
          const rootVersion = rootPeerProvider?.resolvedVersionByPeerName.get(peerName)
          if (rootVersion && satisfies(rootVersion, peerRange)) continue
          if (!issues.missing[peerName]) issues.missing[peerName] = []
          issues.missing[peerName].push({
            parents: currentParents,
            optional: isOptional,
            wantedRange: peerRange,
          })
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

    walkStep(next(), packages, currentParents, issues, rootPeerProvider)
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
          const declaringParent = issue.parents.at(-1)
          if (declaringParent && allowedVersionsByParent[declaringParent.name]?.[peerName]?.some(
            (range) => semver.satisfies(issue.foundVersion, range)
          )) return false
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
