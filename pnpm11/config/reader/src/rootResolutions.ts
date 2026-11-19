import { parsePkgAndParentSelector } from '@pnpm/config.parse-overrides'
import type { ProjectManifest } from '@pnpm/types'

import { assertValidOverrides, createVersionReferencesReplacer } from './getOptionsFromRootManifest.js'
import { sanitizeForLog } from './sanitizeForLog.js'

const MAX_DISPLAYED_RESOLUTIONS = 10

/**
 * Returns the overrides to use given the root manifest's deprecated
 * `resolutions` field. The resolutions become the overrides only when no
 * overrides are configured. Either way a warning is pushed to `warnings`.
 */
export function applyRootResolutions (
  manifest: ProjectManifest,
  overrides: Record<string, string> | undefined,
  warnings: string[]
): Record<string, string> | undefined {
  const resolutions: unknown = manifest.resolutions
  if (resolutions == null) return overrides
  assertValidOverrides(resolutions, 'resolutions')
  if (Object.keys(resolutions).length === 0) return overrides
  if (overrides != null && Object.keys(overrides).length > 0) {
    warnings.push('The "resolutions" field in package.json is ignored because "overrides" in pnpm-workspace.yaml takes precedence. Remove "resolutions" from package.json.')
    return overrides
  }
  const { promoted, entries, skipped } = promoteResolutions(manifest, resolutions)
  warnings.push(formatMigrationWarning(entries, skipped))
  return Object.keys(promoted).length > 0 ? promoted : overrides
}

interface PromotedResolutions {
  promoted: Record<string, string>
  entries: string[]
  skipped: string[]
}

function promoteResolutions (manifest: ProjectManifest, resolutions: Record<string, string>): PromotedResolutions {
  const replaceVersionReference = createVersionReferencesReplacer(manifest)
  const result: PromotedResolutions = { promoted: {}, entries: [], skipped: [] }
  for (const [yarnSelector, original] of Object.entries(resolutions)) {
    const selector = toOverrideSelector(yarnSelector)
    if (selector == null) {
      result.skipped.push(sanitizeForLog(yarnSelector))
      continue
    }
    // `${NAME}` is not a `$name` reference. Resolutions keep it literal.
    const resolved = original.startsWith('${') ? original : replaceVersionReference(original)
    result.entries.push(formatMigratedEntry(selector, original, resolved))
    result.promoted[selector] = resolved
  }
  return result
}

function formatMigratedEntry (selector: string, original: string, resolved: string): string {
  return original === resolved
    ? `  ${sanitizeForLog(selector)}: ${sanitizeForLog(resolved)}`
    : `  ${sanitizeForLog(selector)}: ${sanitizeForLog(original)} -> ${sanitizeForLog(resolved)}`
}

/**
 * Converts a Yarn `resolutions` selector to a pnpm override selector:
 * `**\/foo` becomes `foo` and `parent/foo` becomes `parent>foo`. Returns
 * `undefined` for a path pnpm overrides cannot express, such as `a/**\/foo`
 * or `a/b/foo`. A valid selector in pnpm's own `parent>child` syntax is kept.
 * A selector the override parser rejects is skipped too.
 */
function toOverrideSelector (yarnSelector: string): string | undefined {
  const selector = yarnSelector.includes('>') ? yarnSelector : convertYarnPath(yarnSelector)
  if (selector == null) return undefined
  let parsed: ReturnType<typeof parsePkgAndParentSelector>
  try {
    parsed = parsePkgAndParentSelector(selector)
  } catch {
    return undefined
  }
  const namesOnePackage = (name: string): boolean => splitPackagePath(name)?.length === 1
  return (parsed.parentPkg == null || namesOnePackage(parsed.parentPkg.name)) && namesOnePackage(parsed.targetPkg.name)
    ? selector
    : undefined
}

function convertYarnPath (yarnSelector: string): string | undefined {
  const path = yarnSelector.trim()
  let start = 0
  while (path.startsWith('**/', start)) start += 3
  const packages = splitPackagePath(path.slice(start))
  if (packages?.length === 1) return packages[0]
  if (packages?.length === 2) return `${packages[0]}>${packages[1]}`
  return undefined
}

/**
 * Splits `a/@scope/b` into `['a', '@scope/b']`. `undefined` for a `**` path
 * segment, an empty name, or more than the two packages an override can name.
 * A scoped wildcard such as `@scope/**` is a package name.
 */
function splitPackagePath (path: string): string[] | undefined {
  const segments = path.split('/')
  const packages: string[] = []
  let index = 0
  while (index < segments.length && packages.length < 3) {
    const scoped = segments[index].startsWith('@')
    const pkg = readPackageName(segments, index, scoped)
    if (pkg == null) return undefined
    packages.push(pkg)
    index += scoped ? 2 : 1
  }
  return index < segments.length ? undefined : packages
}

function readPackageName (segments: string[], index: number, scoped: boolean): string | undefined {
  const name = scoped ? segments[index + 1] : segments[index]
  if (!name || (!scoped && name === '**')) return undefined
  return scoped ? `${segments[index]}/${name}` : name
}

function formatMigrationWarning (entries: string[], skipped: string[]): string {
  const skippedNote = skipped.length > 0
    ? `\nThese resolutions have no pnpm override equivalent and were skipped: ${limitDisplayed(skipped).join(', ')}`
    : ''
  return `The "resolutions" field in package.json is deprecated. We attempted to migrate your resolutions to pnpm overrides. Please verify:\n${limitDisplayed(entries).join('\n')}${skippedNote}\nUse the "overrides" field in pnpm-workspace.yaml instead.`
}

function limitDisplayed (items: string[]): string[] {
  if (items.length <= MAX_DISPLAYED_RESOLUTIONS) return items
  return [...items.slice(0, MAX_DISPLAYED_RESOLUTIONS), `  ...and ${items.length - MAX_DISPLAYED_RESOLUTIONS} more`]
}
