import fs from 'node:fs'
import path from 'node:path'

import { createMatcher } from '@pnpm/config.matcher'
import { parseOverrides } from '@pnpm/config.parse-overrides'
import { parse as parseDependencyPath, refToRelative } from '@pnpm/deps.path'
import { getPeerVersionRange } from '@pnpm/deps.peer-range'
import { PnpmError } from '@pnpm/error'
import {
  getLockfileImporterId,
  type LockfileObject,
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
  const importerIds = projectPaths.map((p) => getLockfileImporterId(lockfileDir, p))
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
    checkLinkedDependenciesPeers(importerId, lockfile, lockfileDir, projectIssues)

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
      for (const [peerName, rawPeerRange] of Object.entries(pkgSnapshot.peerDependencies)) {
        const peerRange = getPeerVersionRange(rawPeerRange)
        const isOptional = pkgSnapshot.peerDependenciesMeta?.[peerName]?.optional === true
        const resolvedPeerRef = pkgSnapshot.dependencies?.[peerName] ?? pkgSnapshot.optionalDependencies?.[peerName]

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

    walkStep(next(), packages, currentParents, issues)
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
      conflicts: conflicts.filter((peerName) => filteredMissing[peerName] != null),
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

function checkLinkedDependenciesPeers (
  importerId: string,
  lockfile: LockfileObject,
  lockfileDir: string,
  issues: PeerDependencyIssues
): void {
  const importer = lockfile.importers[importerId as ProjectId]
  if (!importer) return
  const importerDir = path.resolve(lockfileDir, importerId)

  const depGroups = [
    importer.dependencies,
    importer.devDependencies,
    importer.optionalDependencies,
  ]

  let canonicalLockfileDir: string
  try {
    canonicalLockfileDir = fs.realpathSync(lockfileDir)
  } catch {
    canonicalLockfileDir = path.resolve(lockfileDir)
  }

  for (const group of depGroups) {
    if (!group) continue
    for (const [alias, ref] of Object.entries(group as Record<string, string>)) {
      if (!ref.startsWith('link:')) continue
      const linkTarget = ref.slice(5)
      const targetDir = path.resolve(importerDir, linkTarget)

      let canonicalTargetDir: string
      try {
        canonicalTargetDir = fs.realpathSync(targetDir)
      } catch {
        continue
      }
      if (
        canonicalTargetDir !== canonicalLockfileDir &&
        !canonicalTargetDir.startsWith(canonicalLockfileDir + path.sep)
      ) {
        continue
      }

      let manifest: {
        version?: string
        peerDependencies?: Record<string, string>
        peerDependenciesMeta?: Record<string, { optional?: boolean }>
      }
      try {
        manifest = JSON.parse(fs.readFileSync(path.join(canonicalTargetDir, 'package.json'), 'utf8'))
      } catch {
        continue
      }
      if (!manifest.peerDependencies) continue

      const linkedVersion = manifest.version ?? '0.0.0'
      const currentParents: ParentPackages = [{ name: alias, version: linkedVersion }]

      const linkedImporterId = getLockfileImporterId(canonicalLockfileDir, canonicalTargetDir)
      const linkedImporter = lockfile.importers[linkedImporterId as ProjectId]
      const rootImporter = lockfile.importers['.' as ProjectId]

      for (const [peerName, rawPeerRange] of Object.entries(manifest.peerDependencies)) {
        const peerRange = getPeerVersionRange(rawPeerRange)
        const isOptional = manifest.peerDependenciesMeta?.[peerName]?.optional === true

        let foundRef = importer.dependencies?.[peerName] ?? importer.devDependencies?.[peerName] ?? importer.optionalDependencies?.[peerName]
        let foundBaseDir = importerDir

        if (!foundRef && linkedImporter) {
          foundRef = linkedImporter.dependencies?.[peerName] ?? linkedImporter.devDependencies?.[peerName] ?? linkedImporter.optionalDependencies?.[peerName]
          foundBaseDir = canonicalTargetDir
        }

        if (!foundRef && rootImporter && importerId !== '.') {
          foundRef = rootImporter.dependencies?.[peerName] ?? rootImporter.devDependencies?.[peerName] ?? rootImporter.optionalDependencies?.[peerName]
          foundBaseDir = lockfileDir
        }

        if (!foundRef) {
          if (!isOptional) {
            if (!issues.missing[peerName]) issues.missing[peerName] = []
            issues.missing[peerName].push({
              parents: currentParents,
              optional: isOptional,
              wantedRange: peerRange,
            })
          }
          continue
        }

        let foundVersion: string | undefined
        if (foundRef.startsWith('link:')) {
          try {
            const linkedDepDir = path.resolve(foundBaseDir, foundRef.slice(5))
            const linkedDepManifest = JSON.parse(fs.readFileSync(path.join(linkedDepDir, 'package.json'), 'utf8'))
            foundVersion = linkedDepManifest.version
          } catch {
            foundVersion = undefined
          }
        } else {
          foundVersion = extractVersion(foundRef, peerName, lockfile.packages ?? {})
        }

        if (foundVersion && !satisfies(foundVersion, peerRange)) {
          if (!issues.bad[peerName]) issues.bad[peerName] = []
          issues.bad[peerName].push({
            parents: currentParents,
            optional: isOptional,
            wantedRange: peerRange,
            foundVersion,
            resolvedFrom: [],
          })
        }
      }
    }
  }
}
