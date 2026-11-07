import { WorkspaceSpec } from '@pnpm/workspace.spec-parser'
import { inc, prerelease as parsePrerelease } from 'semver'

import type { ReleaseBumpType } from './intents.js'

export const BUMP_ORDER: Record<ReleaseBumpType, number> = { patch: 1, minor: 2, major: 3 }

export function maxBumpType (types: Array<string | undefined>): ReleaseBumpType | null {
  let result: ReleaseBumpType | null = null
  for (const type of types) {
    if (type !== 'patch' && type !== 'minor' && type !== 'major') continue
    if (result == null || BUMP_ORDER[type] > BUMP_ORDER[result]) {
      result = type
    }
  }
  return result
}

export interface NewVersionOptions {
  laneTag?: string
  /**
   * The highest bump accumulated across the package's lane — the
   * planned bump joined with the bumps of intents consumed by earlier
   * prereleases — which keeps the stable target stable across `-tag.N` runs.
   */
  cumulativeBump: ReleaseBumpType
  /** First release: publish `current` verbatim — a stable seed on a lane debuts at its first prerelease. See `unpublishedDirs`. */
  firstRelease: boolean
}

export function computeNewVersion (current: string, bumpType: ReleaseBumpType, opts: NewVersionOptions): string {
  if (opts.laneTag == null) {
    if (opts.firstRelease) return current
    if (parsePrerelease(current) == null) {
      return inc(current, bumpType)!
    }
    // Graduation: the accumulated stable version the lane was
    // building toward.
    return escalateStableTarget(stablePart(current), opts.cumulativeBump)
  }
  if (opts.firstRelease) {
    // A manifest prerelease already on this lane is published verbatim; a
    // stable (or off-lane) seed debuts at the lane's first prerelease.
    return isPrereleaseOnLane(current, opts.laneTag)
      ? current
      : `${stablePart(current)}-${opts.laneTag}.0`
  }
  const target = stableTarget(current, opts.cumulativeBump)
  return `${target}-${opts.laneTag}.${nextPrereleaseNumber(current, target, opts.laneTag)}`
}

/**
 * The stable version a release of `current` by `bumpType` builds toward: the
 * plain increment of a stable version, or the escalated stable part of a
 * prerelease.
 */
export function stableTarget (current: string, bumpType: ReleaseBumpType): string {
  return parsePrerelease(current) == null
    ? inc(current, bumpType)!
    : escalateStableTarget(stablePart(current), bumpType)
}

function isPrereleaseOnLane (version: string, laneTag: string): boolean {
  const prerelease = parsePrerelease(version)
  // semver parses an all-digit identifier as a number, so compare stringified.
  return prerelease != null && String(prerelease[0]) === laneTag
}

/**
 * Re-derives the stable version a lane is building toward when the
 * cumulative bump escalates. The invariant: the stable part of the current
 * prerelease already reflects the previous cumulative bump applied to the
 * version the line started from, so only an escalation changes it.
 */
function escalateStableTarget (target: string, cumulativeBump: ReleaseBumpType): string {
  const [major, minor, patch] = target.split('.').map(Number)
  switch (cumulativeBump) {
    case 'major':
      return minor === 0 && patch === 0 ? target : `${major + 1}.0.0`
    case 'minor':
      return patch === 0 ? target : `${major}.${minor + 1}.0`
    case 'patch':
      return target
  }
}

export function stablePart (version: string): string {
  return version.split('-')[0]
}

export function nextPrereleaseNumber (current: string, target: string, laneTag: string): number {
  const currentPrerelease = parsePrerelease(current)
  if (currentPrerelease == null) return 0
  const [currentTag, currentN] = currentPrerelease
  // semver parses an all-digit prerelease identifier as a number, so the tag
  // comparison must not be strict about the type.
  if (stablePart(current) !== target || String(currentTag) !== laneTag || typeof currentN !== 'number') return 0
  return currentN + 1
}

/**
 * The range that pnpm materializes for a workspace: spec at pack time, given
 * the dependency's version at the dependent's previous release. Dependent
 * propagation republishes the dependent whenever the dependency's new version
 * falls outside this range.
 */
export function materializeWorkspaceRange (spec: string, depCurrentVersion: string): string | null {
  const parsed = WorkspaceSpec.parse(spec)
  if (parsed == null) return null
  switch (parsed.version) {
    case '^':
      return `^${depCurrentVersion}`
    case '~':
      return `~${depCurrentVersion}`
    case '*':
    case '':
      return depCurrentVersion
    default:
      return parsed.version
  }
}
