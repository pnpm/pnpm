import { createMatcherWithIndex, type MatcherWithIndex } from '@pnpm/config.matcher'
import { PnpmError } from '@pnpm/error'
import type { UpdateMatchingFunction } from '@pnpm/installing.deps-installer'
import { globalWarn } from '@pnpm/logger'
import { filterDependenciesByType } from '@pnpm/pkg-manifest.utils'
import type { IncludedDependencies, ProjectManifest } from '@pnpm/types'
import getVersionSelectorType from 'version-selector-type'

export function matchDependencies (
  match: (input: string) => string | null,
  manifest: ProjectManifest,
  include: IncludedDependencies
): string[] {
  const deps = Object.keys(filterDependenciesByType(manifest, include))
  const matchedDeps = []
  for (const dep of deps) {
    const spec = match(dep)
    if (spec === null) continue
    matchedDeps.push(spec ? `${dep}@${spec}` : dep)
  }
  return matchedDeps
}

/**
 * The update-target predicate of `pnpm update <selector>...`. The version part
 * of an exact selector narrows which resolved copies of a matched package are
 * update targets: `foo@1.2.3` targets only the version line that can resolve to
 * `1.2.3` — the same major, or the same minor when the request is on `0.x`,
 * where the minor is the compatibility boundary. A package the workspace
 * depends on twice therefore keeps the copies on its other lines untouched.
 *
 * A selector that carries a range, a tag, or no version at all targets by name
 * alone, and so does every call made before the edge's resolved version is
 * known. Negated selectors exclude names, never versions.
 */
export function createUpdateMatching (params: string[]): UpdateMatchingFunction {
  const parsed = params.map(parseUpdateParam)
  const matchesAnySelector = createMatcherWithIndex(parsed.map(({ pattern }) => pattern))
  const versionScopes = parsed
    .filter(({ pattern }) => pattern[0] !== '!')
    .map(({ pattern, versionSpec }) => ({
      matchesPattern: createMatcherWithIndex([pattern]),
      requestedLine: versionSpec != null ? parseVersionLine(versionSpec) : undefined,
    }))
  return (pkgName: string, version?: string) => {
    if (matchesAnySelector(pkgName) === -1) return false
    if (versionScopes.length === 0) return true
    return versionScopes.some((versionScope) => isInVersionScope(versionScope, pkgName, version))
  }
}

interface VersionScope {
  matchesPattern: MatcherWithIndex
  requestedLine: VersionLine | undefined
}

interface VersionLine {
  major: number
  minor: number
}

function isInVersionScope (
  { matchesPattern, requestedLine }: VersionScope,
  pkgName: string,
  version: string | undefined
): boolean {
  if (matchesPattern(pkgName) === -1) return false
  if (requestedLine == null || version == null) return true
  const currentLine = parseVersionLine(version)
  if (currentLine == null || currentLine.major !== requestedLine.major) return false
  return requestedLine.major !== 0 || currentLine.minor === requestedLine.minor
}

/**
 * The version a selector names, normalized, or `undefined` for a range, a tag
 * or an `npm:` alias spec — none of which name a single version.
 */
function parseExactVersion (versionSpec: string): string | undefined {
  const selector = getVersionSelectorType(versionSpec)
  return selector?.type === 'version' ? selector.normalized : undefined
}

/** The major and minor of the version a selector names, if it names one. */
function parseVersionLine (versionSpec: string): VersionLine | undefined {
  const version = parseExactVersion(versionSpec)
  if (version == null) return undefined
  const [major, minor] = version.split('.')
  return { major: Number(major), minor: Number(minor) }
}

/**
 * `pnpm update <dep>@<version>` where `<dep>` matches no direct dependency has
 * nowhere to record the version. An update resolves such a target the same way
 * a fresh install would — which a command-line version cannot influence — so
 * honoring the request would mean writing a lockfile entry no manifest backs,
 * and the next fresh resolve would undo it. Neither npm nor Yarn accepts a
 * version here either. Fail rather than resolve to something else and leave
 * the caller a zero exit status to read.
 *
 * A range or a tag is not held to the same standard: it names no single
 * version to record, and updating within the dependents' ranges is a
 * reasonable reading of it. Those keep the warning.
 *
 * The override the hint recommends is scoped to the dependents' declared range
 * so it cannot violate any consumer's range; that range lives in the
 * dependents' manifests, which this layer does not read, hence the
 * placeholder.
 */
export function failOnVersionsOfIndirectUpdateSpecs (
  updateSpecs: string[],
  manifests: ProjectManifest[],
  include: IncludedDependencies
): void {
  const pinned: Array<{ pattern: string, version: string }> = []
  for (const spec of updateSpecs) {
    const { pattern, versionSpec } = parseUpdateParam(spec)
    // A negated selector excludes names; a version on one asks for nothing.
    if (versionSpec == null || pattern[0] === '!') continue
    if (matchesADirectDependency(pattern, manifests, include)) continue
    const version = parseExactVersion(versionSpec)
    if (version == null) {
      globalWarn(`"${pattern}" is not a direct dependency, so the requested "${versionSpec}" is ignored — "${pattern}" is updated to what a fresh install would resolve.`)
      continue
    }
    pinned.push({ pattern, version })
  }
  if (pinned.length === 0) return
  const subjects = pinned.map(({ pattern, version }) => `"${pattern}" (requested "${version}")`)
  const overrides = pinned.map(({ pattern, version }) => `    ${pattern}@<declared range>: ${version}`)
  throw new PnpmError('UPDATE_VERSION_ON_INDIRECT_DEP',
    `${subjects.join(', ')} ${pinned.length === 1 ? 'is not a direct dependency, so the requested version cannot' : 'are not direct dependencies, so the requested versions cannot'} be recorded.`,
    {
      hint: `An update resolves a transitive dependency the way a fresh install would, so a version on the command line has no effect on it. To pin one, add an override scoped to the range its dependents declare to pnpm-workspace.yaml:

  overrides:
${overrides.join('\n')}

To update it within the range its dependents already declare, drop the version: pnpm update ${pinned.map(({ pattern }) => pattern).join(' ')}`,
    })
}

/**
 * Whether any of `manifests` declares a dependency `pattern` names, so the
 * update has a manifest entry to write the requested version into. A pattern
 * that matches nothing directly reaches its target only through the resolver,
 * which the version cannot steer.
 */
function matchesADirectDependency (
  pattern: string,
  manifests: ProjectManifest[],
  include: IncludedDependencies
): boolean {
  const match = createMatcher([pattern])
  return manifests.some((manifest) => matchDependencies(match, manifest, include).length > 0)
}

export type UpdateDepsMatcher = (input: string) => string | null

export function createMatcher (params: string[]): UpdateDepsMatcher {
  const patterns: string[] = []
  const specs: string[] = []
  for (const param of params) {
    const { pattern, versionSpec } = parseUpdateParam(param)
    patterns.push(pattern)
    specs.push(versionSpec ?? '')
  }
  const matcher = createMatcherWithIndex(patterns)
  return (depName: string) => {
    const index = matcher(depName)
    if (index === -1) return null
    return specs[index]
  }
}

export function parseUpdateParam (param: string): { pattern: string, versionSpec: string | undefined } {
  const atIndex = param.indexOf('@', param[0] === '!' ? 2 : 1)
  if (atIndex === -1) {
    return {
      pattern: param,
      versionSpec: undefined,
    }
  }
  return {
    pattern: param.slice(0, atIndex),
    versionSpec: param.slice(atIndex + 1),
  }
}

/**
 * The selectors an update selector stands for. An `npm:` selector contributes
 * a second one for the aliased package, because that — not the alias — is the
 * name the resolver resolves the edge under; it carries the aliased spec's own
 * version so the expansion scopes the same version line the user asked for.
 */
export function expandUpdateSelectorsForMatching (selector: string): string[] {
  const { pattern, versionSpec } = parseUpdateParam(selector)
  if (versionSpec?.startsWith('npm:') !== true) return [selector]
  const aliasSelector = parseUpdateParam(versionSpec.slice('npm:'.length))
  const aliasPattern = pattern[0] === '!' ? `!${aliasSelector.pattern}` : aliasSelector.pattern
  const aliasSpec = aliasSelector.versionSpec != null ? `${aliasPattern}@${aliasSelector.versionSpec}` : aliasPattern
  return [selector, aliasSpec]
}

export function makeIgnorePatterns (ignoredDependencies: string[]): string[] {
  return ignoredDependencies.map(depName => `!${depName}`)
}
