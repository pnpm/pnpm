import * as dp from '@pnpm/deps.path'
import type { LockfileObject, PackageSnapshots, ProjectId, ProjectSnapshot, ResolvedDependencies } from '@pnpm/lockfile.fs'

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
      addResolvedDeps(keys, importer[field], lockfile.packages)
    }
  }
  return keys
}

function addResolvedDeps (keys: Set<string>, deps: ResolvedDependencies | undefined, packages: PackageSnapshots | undefined): void {
  if (deps == null) return
  for (const [alias, ref] of Object.entries(deps)) {
    const key = resolveDepKey(alias, ref, packages)
    if (key) keys.add(key)
  }
}

// Resolves an importer's dependency reference to the `name@version` identity
// the store scanner reports packages under. `ref` is the raw resolved
// reference from the lockfile (e.g. "5.0.10" for a same-name dependency,
// "is-positive@1.0.0" for a dependency installed under an `npm:` alias, or a
// git/tarball/`file:` URL). `dp.refToRelative` turns the (ref, alias) pair into
// a dep path using the same resolution pnpm relies on elsewhere for importer
// dependency refs (e.g. the virtual node_modules builder). It returns null for
// `link:` refs, which point at workspace projects (not store packages) and so
// are skipped. For `file:`/git/tarball refs `dp.parse` yields only a
// `nonSemverVersion` (the raw URL/id), but the scanner reports those packages
// under their real semver — held on the package snapshot's `version` field — so
// we prefer that snapshot version before falling back to the non-semver id.
function resolveDepKey (alias: string, ref: string, packages: PackageSnapshots | undefined): string | undefined {
  const depPath = dp.refToRelative(ref, alias)
  if (depPath == null) return undefined
  const { name, version, nonSemverVersion } = dp.parse(depPath)
  const resolvedVersion = version ?? packages?.[depPath]?.version ?? nonSemverVersion
  if (!name || !resolvedVersion) return undefined
  return `${name}@${resolvedVersion}`
}
