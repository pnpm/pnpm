import { packageNameFromAllowBuildKey, UNDECIDED_ALLOW_BUILD } from '@pnpm/building.policy'
import { mergePackageVersionSpecs, parseVersionPolicyRule } from '@pnpm/config.version-policy'
import type { WorkspaceManifest } from '@pnpm/workspace.workspace-manifest-reader'

export type ExcludeListField = 'minimumReleaseAgeExclude' | 'trustPolicyExclude'

type ResolvedPackageVersions = ReadonlyMap<string, ReadonlySet<string>>

interface PrunedExcludeEntry {
  survivingSpecs: string[]
  changed: boolean
}

// The `minimumReleaseAgeExcludePrune` / `trustPolicyExcludePrune` pass over
// the exclude list `field` names. An entry is dropped when the freshly
// resolved lockfile no longer contains what it names: exact versions that
// were not resolved are dropped (the entry goes away once none remain), and a
// bare-name entry goes away when the package is absent entirely. Glob patterns
// always stay — they are forward-looking and can't be proven stale. Entries
// that fail to parse stay untouched so cleanup never breaks an install.
export function pruneExcludeList (
  manifest: Partial<WorkspaceManifest> & { [key in ExcludeListField]?: string[] },
  field: ExcludeListField,
  resolvedPackageVersions: ResolvedPackageVersions
): boolean {
  const excludes = manifest[field]
  if (excludes == null || excludes.length === 0) {
    return false
  }
  const survivingSpecs: string[] = []
  let changed = false
  for (const entry of excludes) {
    const prunedEntry = pruneExcludeEntry(entry, resolvedPackageVersions)
    survivingSpecs.push(...prunedEntry.survivingSpecs)
    changed = prunedEntry.changed || changed
  }
  if (!changed) {
    return false
  }
  if (survivingSpecs.length === 0) {
    delete manifest[field]
  } else {
    manifest[field] = survivingSpecs
  }
  return true
}

function pruneExcludeEntry (entry: string, resolvedPackageVersions: ResolvedPackageVersions): PrunedExcludeEntry {
  const rule = tryParseVersionPolicyRule(entry)
  if (rule == null) {
    return { survivingSpecs: [entry], changed: false }
  }
  const { packageName, exactVersions } = rule
  if (exactVersions.length === 0) {
    const isStillRelevant = packageName.includes('*') || resolvedPackageVersions.has(packageName)
    return isStillRelevant
      ? { survivingSpecs: [entry], changed: false }
      : { survivingSpecs: [], changed: true }
  }
  const resolved = resolvedPackageVersions.get(packageName)
  const survivingVersions = exactVersions.filter((version) => resolved?.has(version))
  if (survivingVersions.length === exactVersions.length) {
    return { survivingSpecs: [entry], changed: false }
  }
  if (survivingVersions.length === 0) {
    return { survivingSpecs: [], changed: true }
  }
  return {
    survivingSpecs: mergePackageVersionSpecs([`${packageName}@${survivingVersions.join(' || ')}`]),
    changed: true,
  }
}

function tryParseVersionPolicyRule (entry: string): ReturnType<typeof parseVersionPolicyRule> | undefined {
  try {
    return parseVersionPolicyRule(entry)
  } catch {
    return undefined
  }
}

// Drops undecided placeholder entries whose package is provably absent from
// the resolved lockfile. Explicit decisions, keys with no provable package
// name, and entries for still-resolved packages always stay.
export function pruneAllowBuilds (
  manifest: Partial<WorkspaceManifest>,
  resolvedPackageVersions: ResolvedPackageVersions
): boolean {
  const allowBuilds = manifest.allowBuilds
  if (allowBuilds == null) {
    return false
  }
  let changed = false
  for (const [key, value] of Object.entries(allowBuilds)) {
    if (value !== UNDECIDED_ALLOW_BUILD) continue
    const packageName = packageNameFromAllowBuildKey(key)
    if (packageName == null || resolvedPackageVersions.has(packageName)) continue
    delete allowBuilds[key]
    changed = true
  }
  if (changed && Object.keys(allowBuilds).length === 0) {
    delete manifest.allowBuilds
  }
  return changed
}
