import { calcVersionRange, inferRangeSpecStyle, rangeSpecGranularity, versionWithRangeSpecStyle } from '@pnpm/pkg-manifest.utils'
import type { WantedDependency } from '@pnpm/resolving.resolver-base'
import type { RangeSpecStyle } from '@pnpm/types'
import semver from 'semver'
import versionSelectorType from 'version-selector-type'

import type { RegistryPackageSpec } from './parseBareSpecifier.js'

// Builds a `<prefix><pkgName>@<range>` specifier (or a bare `<prefix><range>`
// when the dependency alias matches the package name). Shared between the
// jsr and named-registry resolvers since they only differ in `prefix` and
// which spec field holds the package name.
export function calcPrefixedSpecifier (opts: {
  prefix: string
  pkgName: string
  wantedDependency: WantedDependency
  version: string
  revision?: number
  defaultRangeSpecStyle?: RangeSpecStyle
  isUpdate?: boolean
}): string {
  if (opts.revision != null) {
    const target = `${opts.version}+r${opts.revision}`
    if (isSavedUnderOwnName(opts.wantedDependency, opts.pkgName)) return `${opts.prefix}${target}`
    return `${opts.prefix}${opts.pkgName}@${target}`
  }
  const range = calcRange(opts.version, opts.wantedDependency, opts.defaultRangeSpecStyle, opts.isUpdate)
  if (isSavedUnderOwnName(opts.wantedDependency, opts.pkgName)) return `${opts.prefix}${range}`
  return `${opts.prefix}${opts.pkgName}@${range}`
}

export function calcSpecifier ({
  wantedDependency,
  spec,
  version,
  defaultRangeSpecStyle,
  isUpdate,
}: {
  wantedDependency: WantedDependency
  spec: RegistryPackageSpec
  version: string
  defaultRangeSpecStyle?: RangeSpecStyle
  isUpdate?: boolean
}): string {
  if (spec.revision != null) {
    const target = `${version}+r${spec.revision}`
    if (isSavedUnderOwnName(wantedDependency, spec.name)) return target
    return `npm:${spec.name}@${target}`
  }
  if (wantedDependency.prevSpecifier === wantedDependency.bareSpecifier && wantedDependency.prevSpecifier && versionSelectorType(wantedDependency.prevSpecifier)?.type === 'tag') {
    return wantedDependency.prevSpecifier
  }
  const range = calcRange(version, wantedDependency, defaultRangeSpecStyle, isUpdate)
  if (isSavedUnderOwnName(wantedDependency, spec.name)) return range
  return `npm:${spec.name}@${range}`
}

/** The manifest range `version` is saved as; see {@link calcVersionRange}. */
function calcRange (version: string, wantedDependency: WantedDependency, defaultRangeSpecStyle?: RangeSpecStyle, isUpdate?: boolean): string {
  return calcVersionRange(version, {
    prevSpecifier: wantedDependency.prevSpecifier,
    bareSpecifier: wantedDependency.bareSpecifier,
    defaultRangeSpecStyle,
    isUpdate,
  })
}

/** Whether the dependency needs no alias prefix to name `pkgName`. */
function isSavedUnderOwnName (wantedDependency: WantedDependency, pkgName: string): boolean {
  return !wantedDependency.alias || pkgName === wantedDependency.alias
}

interface WorkspaceDepSpecifierOptions {
  wantedDependency: WantedDependency
  spec: RegistryPackageSpec
  saveWorkspaceProtocol: boolean | 'rolling' | undefined
  // A workspace project may omit its version, whatever its manifest type says.
  version: string | undefined
  defaultRangeSpecStyle?: RangeSpecStyle
  isUpdate?: boolean
}

export function calcSpecifierForWorkspaceDep (opts: WorkspaceDepSpecifierOptions): string {
  const { wantedDependency, spec, saveWorkspaceProtocol, version } = opts
  const parsedVersion = semver.parse(version)
  if (version != null && !saveWorkspaceProtocol && !wantedDependency.bareSpecifier?.startsWith('workspace:')) {
    const specifierWithoutProtocol = calcSpecifierWithoutWorkspaceProtocol(opts, version, parsedVersion)
    if (specifierWithoutProtocol != null) return specifierWithoutProtocol
  }
  const prefix = isSavedUnderOwnName(wantedDependency, spec.name) ? 'workspace:' : `workspace:${spec.name}@`
  if (saveWorkspaceProtocol === 'rolling' || version == null) {
    return calcRollingWorkspaceSpecifier(prefix, wantedDependency)
  }
  return calcPinnedWorkspaceSpecifier({ ...opts, prefix, version, parsedVersion })
}

function calcSpecifierWithoutWorkspaceProtocol (
  opts: WorkspaceDepSpecifierOptions,
  version: string,
  parsedVersion: semver.SemVer | null
): string | undefined {
  const { wantedDependency, spec } = opts
  if (parsedVersion != null) {
    return calcSpecifier({
      wantedDependency,
      spec,
      version,
      defaultRangeSpecStyle: opts.defaultRangeSpecStyle,
      isUpdate: opts.isUpdate,
    })
  }
  if (!isPartialVersion(version)) return undefined
  return isSavedUnderOwnName(wantedDependency, spec.name) ? version : `npm:${spec.name}@${version}`
}

function calcRollingWorkspaceSpecifier (prefix: string, wantedDependency: WantedDependency): string {
  const specifier = wantedDependency.prevSpecifier ?? wantedDependency.bareSpecifier
  if (!specifier) return `${prefix}^`
  if ([`${prefix}*`, `${prefix}^`, `${prefix}~`].includes(specifier)) return specifier
  const rangeSpecStyle = inferRangeSpecStyle(specifier)
  switch (rangeSpecStyle && rangeSpecGranularity(rangeSpecStyle)) {
    case 'major': return `${prefix}^`
    case 'minor': return `${prefix}~`
    case 'patch':
    case 'none': return `${prefix}*`
    case undefined: break
  }
  return `${prefix}^`
}

function calcPinnedWorkspaceSpecifier (opts: WorkspaceDepSpecifierOptions & {
  prefix: string
  version: string
  parsedVersion: semver.SemVer | null
}): string {
  const { prefix, version, wantedDependency, isUpdate } = opts
  const prevRangeSpecStyle = inferOptionalRangeSpecStyle(wantedDependency.prevSpecifier)
  const requestedRangeSpecStyle = inferOptionalRangeSpecStyle(wantedDependency.bareSpecifier)
  if (!isUpdate && (requestedRangeSpecStyle === 'patch' || requestedRangeSpecStyle === 'exact')) {
    return `${prefix}${versionWithRangeSpecStyle(version, requestedRangeSpecStyle)}`
  }
  if (isPartialOrPrereleaseVersion(version, opts.parsedVersion)) {
    return prevRangeSpecStyle ? `${prefix}${versionWithRangeSpecStyle(version, prevRangeSpecStyle)}` : `${prefix}${version}`
  }
  const rangeSpecStyle = isUpdate
    ? (prevRangeSpecStyle ?? requestedRangeSpecStyle ?? opts.defaultRangeSpecStyle)
    : (requestedRangeSpecStyle ?? prevRangeSpecStyle ?? opts.defaultRangeSpecStyle)
  const range = versionWithRangeSpecStyle(version, rangeSpecStyle ?? 'major')
  return `${prefix}${range}`
}

function inferOptionalRangeSpecStyle (specifier: string | undefined): RangeSpecStyle | undefined {
  return specifier ? inferRangeSpecStyle(specifier) : undefined
}

function isPartialOrPrereleaseVersion (version: string, parsedVersion: semver.SemVer | null): boolean {
  return parsedVersion == null ? isPartialVersion(version) : parsedVersion.prerelease.length > 0
}

/**
 * `1`, `1.0` or `1.x`: a non-semver workspace version that is saved exactly,
 * with or without the `workspace:` protocol, since a `^`/`~` range over it
 * would not match it. Any other non-semver version keeps the operator: written
 * exactly it could mean a wildcard, a tag or an alias inside `workspace:`, or
 * a different dependency source without it.
 */
function isPartialVersion (version: string): boolean {
  const [major, ...minorAndPatch] = version.split('.')
  return isVersionNumber(major) &&
    minorAndPatch.length <= 2 &&
    minorAndPatch.every((part) => ['x', 'X', '*'].includes(part) || isVersionNumber(part)) &&
    semver.validRange(version) != null
}

function isVersionNumber (part: string): boolean {
  return part === '0' || (part !== '' && !part.startsWith('0') && [...part].every((char) => char >= '0' && char <= '9'))
}
