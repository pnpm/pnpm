import { stripVTControlCharacters } from 'node:util'

import type { DevEngines, EngineDependency, ProjectManifest } from '@pnpm/types'
import semver from 'semver'

import type { WantedPackageManager } from '../Config.js'

interface WantedPackageManagerResult {
  pm?: WantedPackageManager
  warnings: string[]
}

export function getWantedPackageManager (manifest: ProjectManifest): WantedPackageManagerResult {
  const pmFromDevEngines = parseDevEnginesPackageManager(manifest.devEngines)
  if (pmFromDevEngines) {
    return getWantedPackageManagerFromDevEngines(manifest, pmFromDevEngines)
  }
  if (manifest.packageManager) {
    return getWantedPackageManagerFromLegacyField(manifest.packageManager)
  }
  return { warnings: [] }
}

function getWantedPackageManagerFromDevEngines (manifest: ProjectManifest, pmFromDevEngines: EngineDependency): WantedPackageManagerResult {
  const warnings: string[] = []
  if (pmFromDevEngines.version != null && !semver.validRange(pmFromDevEngines.version)) {
    warnings.push(`Cannot use devEngines.packageManager version "${pmFromDevEngines.version}": not a valid version or range`)
    pmFromDevEngines.version = undefined
  }
  if (manifest.packageManager) {
    const legacyPm = parsePackageManager(manifest.packageManager)
    const conflictWarning = getPackageManagerConflictWarning(legacyPm, {
      name: pmFromDevEngines.name,
      ...splitPackageManagerVersion(pmFromDevEngines.version),
    })
    if (conflictWarning) {
      warnings.push(conflictWarning)
    }
  }
  return { pm: { ...pmFromDevEngines, fromDevEngines: true }, warnings }
}

function getWantedPackageManagerFromLegacyField (packageManager: string): WantedPackageManagerResult {
  const warnings: string[] = []
  const pm = parsePackageManager(packageManager)
  if (pm.version != null) {
    const cleanVersion = semver.valid(pm.version)
    if (!cleanVersion) {
      warnings.push(`Cannot use packageManager "${packageManager}": "${pm.version}" is not a valid exact version`)
      pm.version = undefined
    } else if (cleanVersion !== pm.version) {
      warnings.push(`Cannot use packageManager "${packageManager}": you need to specify the version as "${cleanVersion}"`)
      pm.version = undefined
    }
  }
  return { pm, warnings }
}

export interface ParsedPackageManager {
  name: string
  version: string | undefined
  hash: string | undefined
}

export function parsePackageManager (packageManager: string): ParsedPackageManager {
  // Split on the `@` that separates the name from the reference. A leading `@`
  // belongs to a scoped name (e.g. `@scope/pm@1.2.3`), so skip it; otherwise
  // the first `@` is the separator. The first `@` (not the last) is used so a
  // reference that is a URL containing `@` (e.g. credentials) stays intact.
  const separatorIndex = packageManager.startsWith('@')
    ? packageManager.indexOf('@', 1)
    : packageManager.indexOf('@')
  if (separatorIndex === -1) return { name: packageManager, version: undefined, hash: undefined }
  const name = packageManager.slice(0, separatorIndex)
  const pmReference = packageManager.slice(separatorIndex + 1)
  // pmReference is semantic versioning, not URL
  if (pmReference.includes(':')) return { name, version: undefined, hash: undefined }
  return { name, ...splitPackageManagerVersion(pmReference) }
}

/**
 * Splits a package manager version reference into its semver part and the
 * integrity hash carried as semver build metadata, e.g.
 * "9.5.0+sha512.140036830124618d624a2187b50d04289d5a087f326c9edfc0ccd733d76c4f52c3a313d4fc148794a2a9d81553016004e6742e8cf850670268a7387fc220c903"
 * becomes `{ version: "9.5.0", hash: "sha512.14003..." }`. A reference without a
 * hash yields an undefined hash; an undefined reference yields both undefined.
 */
function splitPackageManagerVersion (reference: string | undefined): { version: string | undefined, hash: string | undefined } {
  if (reference == null) return { version: undefined, hash: undefined }
  // Split on the first `+` only. The integrity hash is semver build metadata —
  // everything after that `+` — and must be preserved whole, so a reference is
  // never truncated at a later `+`.
  const hashIndex = reference.indexOf('+')
  if (hashIndex === -1) return { version: reference, hash: undefined }
  return { version: reference.slice(0, hashIndex), hash: reference.slice(hashIndex + 1) }
}

/**
 * Describes how the legacy `packageManager` field disagrees with
 * `devEngines.packageManager`, or returns undefined when the two specifiers are
 * identical (so keeping both fields in sync produces no warning). Any
 * divergence warns — including an integrity hash (semver build metadata) on
 * only one side, since dropping the ignored `packageManager` field would lose
 * it. In every conflict `devEngines.packageManager` wins and `packageManager`
 * is ignored.
 */
function getPackageManagerConflictWarning (legacy: ParsedPackageManager, devEngines: ParsedPackageManager): string | undefined {
  const ignoredSuffix = '. "packageManager" will be ignored'
  const genericWarning = `Cannot use both "packageManager" and "devEngines.packageManager" in package.json${ignoredSuffix}`
  if (legacy.name !== devEngines.name) {
    return `"packageManager" (${sanitizeManifestValue(legacy.name)}) and "devEngines.packageManager" (${sanitizeManifestValue(devEngines.name)}) specify different package managers in package.json${ignoredSuffix}`
  }
  if (legacy.version !== devEngines.version) {
    // "different versions" only makes sense when both sides are concrete
    // versions. If one side has no semver version — e.g. the legacy field is a
    // URL or a bare name — fall back to the generic notice rather than claiming
    // a version mismatch.
    if (legacy.version == null || devEngines.version == null) return genericWarning
    return `"packageManager" and "devEngines.packageManager" specify different versions of ${sanitizeManifestValue(legacy.name)} in package.json${ignoredSuffix}`
  }
  if (legacy.hash !== devEngines.hash) {
    // Same name and version, but the integrity hashes differ. Two distinct
    // hashes for one version is a likely wrong-hash mistake, so call it out
    // specifically; a hash on only one side is a softer mismatch (the version
    // still agrees) and gets the generic notice.
    if (legacy.hash != null && devEngines.hash != null && legacy.version != null) {
      return `"packageManager" and "devEngines.packageManager" specify ${sanitizeManifestValue(legacy.name)}@${sanitizeManifestValue(legacy.version)} with different integrity hashes in package.json${ignoredSuffix}`
    }
    return genericWarning
  }
  return undefined
}

/**
 * Renders a package.json-controlled value safe to embed in a warning printed to
 * the terminal. Strips ANSI escape sequences and replaces remaining control
 * characters (including newlines) with spaces so a malicious manifest cannot
 * forge or rewrite terminal/CI log output.
 */
function sanitizeManifestValue (value: string): string {
  // eslint-disable-next-line no-control-regex -- matching control characters is the point
  return stripVTControlCharacters(value).replace(/[\u0000-\u001f\u007f]/g, ' ')
}

/**
 * Decides whether the resolved pnpm integrity info should be written to
 * `pnpm-lock.yaml` under the project's `packageManagerDependencies` section.
 *
 * `onFail: ignore` means pnpm should not enforce or record the package manager
 * policy. Otherwise, `devEngines.packageManager` persists because it may use
 * ranges, while the legacy `packageManager` field only persists for pnpm v12+.
 */
export function shouldPersistLockfile (pm: Pick<WantedPackageManager, 'version' | 'fromDevEngines' | 'onFail'>): boolean {
  if (pm.onFail === 'ignore') return false
  if (pm.fromDevEngines === true) return true
  if (pm.version == null || semver.valid(pm.version) == null) return false
  return semver.major(pm.version) >= 12
}

function parseDevEnginesPackageManager (devEngines?: DevEngines): EngineDependency | undefined {
  if (!devEngines?.packageManager) return undefined
  const selected = Array.isArray(devEngines.packageManager)
    ? selectPackageManagerEngine(devEngines.packageManager)
    // Singular form: leave onFail undefined when the user did not set it, so
    // the central pmOnFail default ('download') applies. The array form keeps
    // its own per-element defaults ('error' for the last entry, 'ignore' for
    // the rest) because those reflect explicit prioritization by the user.
    : { pmEngine: devEngines.packageManager, onFail: devEngines.packageManager.onFail }
  if (!selected?.pmEngine?.name) return undefined
  return {
    name: selected.pmEngine.name,
    version: selected.pmEngine.version,
    onFail: selected.onFail,
  }
}

interface SelectedPackageManagerEngine {
  pmEngine: EngineDependency | undefined
  onFail: EngineDependency['onFail']
}

function selectPackageManagerEngine (engines: EngineDependency[]): SelectedPackageManagerEngine | undefined {
  if (engines.length === 0) return undefined
  const pnpmIndex = engines.findIndex((engine) => engine.name === 'pnpm')
  if (pnpmIndex !== -1) {
    const pmEngine = engines[pnpmIndex]
    // In array notation, default onFail is 'error' for the last element, 'ignore' for others.
    return { pmEngine, onFail: pmEngine.onFail ?? (pnpmIndex === engines.length - 1 ? 'error' : 'ignore') }
  }
  // No pnpm entry found — use the last element's onFail for the overall failure behavior.
  const lastEngine = engines[engines.length - 1]
  return { pmEngine: engines[0], onFail: lastEngine.onFail ?? 'error' }
}

export function getNodeVersionFromEnginesRuntime (manifest: ProjectManifest): string | undefined {
  for (const enginesFieldName of ['devEngines', 'engines'] as const) {
    const resolved = resolveNodeVersionFromEnginesRuntime(manifest[enginesFieldName]?.runtime)
    if (resolved != null) return resolved.version
  }
  return undefined
}

/**
 * Returns undefined when this runtime field names no usable Node.js version,
 * so the next field is consulted. A defined result ends the search, even when
 * the version it carries is undefined.
 */
function resolveNodeVersionFromEnginesRuntime (
  enginesRuntime: DevEngines['runtime']
): { version: string | undefined } | undefined {
  if (enginesRuntime == null) return undefined
  const runtimes: EngineDependency[] = Array.isArray(enginesRuntime) ? enginesRuntime : [enginesRuntime]
  const nodeRuntime = runtimes.find((runtime) => runtime.name === 'node')
  if (typeof nodeRuntime?.version !== 'string') return undefined
  const version = nodeRuntime.version.trim()
  if (!semver.validRange(version)) return undefined
  if (nodeRuntime.onFail !== 'download') {
    return { version: semver.valid(version) ?? undefined }
  }
  const minVersion = semver.minVersion(version)
  return minVersion != null ? { version: minVersion.version } : undefined
}
