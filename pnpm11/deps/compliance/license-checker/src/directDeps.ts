import * as dp from '@pnpm/deps.path'
import type { LockfileObject, ProjectId, ProjectSnapshot, ResolvedDependencies } from '@pnpm/lockfile.fs'

const DEP_FIELDS: Array<keyof Pick<ProjectSnapshot, 'dependencies' | 'devDependencies' | 'optionalDependencies'>> = [
  'dependencies',
  'devDependencies',
  'optionalDependencies',
]

/**
 * Derives the set of `name@version` identities directly depended on by the
 * given importers (all importers when `importerIds` is undefined), resolved
 * through the lockfile so `npm:` aliases map to their real package name.
 * Scanned packages are matched against this set by `${pkg.name}@${pkg.version}`.
 */
export function collectDirectDepKeys (lockfile: LockfileObject, importerIds?: string[]): Set<string> {
  const keys = new Set<string>()
  const ids = importerIds ?? Object.keys(lockfile.importers ?? {})
  for (const id of ids) {
    const importer = lockfile.importers?.[id as ProjectId]
    if (importer == null) continue
    for (const field of DEP_FIELDS) {
      addResolvedDeps(keys, importer[field])
    }
  }
  return keys
}

function addResolvedDeps (keys: Set<string>, deps?: ResolvedDependencies): void {
  if (deps == null) return
  for (const [alias, ref] of Object.entries(deps)) {
    const key = resolveDepKey(alias, ref)
    if (key) keys.add(key)
  }
}

// Resolves an importer's dependency reference to the `name@version` identity
// the store scanner reports packages under. `ref` is the raw resolved
// reference from the lockfile (e.g. "5.0.10" for a same-name dependency, or
// "is-positive@1.0.0" for a dependency installed under an `npm:` alias).
// `dp.refToRelative` turns the (ref, alias) pair into a dep path using the
// same resolution pnpm relies on elsewhere for importer dependency refs
// (e.g. the virtual node_modules builder); it returns null for `link:`
// refs, which point at workspace projects rather than store packages.
function resolveDepKey (alias: string, ref: string): string | undefined {
  const depPath = dp.refToRelative(ref, alias)
  if (depPath == null) return undefined
  const { name, version, nonSemverVersion } = dp.parse(depPath)
  const resolvedVersion = version ?? nonSemverVersion
  if (!name || !resolvedVersion) return undefined
  return `${name}@${resolvedVersion}`
}
