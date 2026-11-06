import path from 'node:path'

interface ImporterDependency {
  ref: string
  /** The directory of the importer that records the dependency. */
  rootDir: string
}

/**
 * Whether the workspace root's `rootDependency` resolves to what `dependency`
 * resolves to, so `dedupeDirectDeps` leaves it to the root. A `link:` reference
 * is relative to its importer, so links compare by the directory they point at,
 * as the link pass does.
 */
export function isProvidedByRoot (dependency: ImporterDependency, rootDependency: ImporterDependency): boolean {
  if (dependency.ref.startsWith('link:') && rootDependency.ref.startsWith('link:')) {
    return linkTargetDir(dependency) === linkTargetDir(rootDependency)
  }
  return dependency.ref === rootDependency.ref
}

function linkTargetDir ({ ref, rootDir }: ImporterDependency): string {
  return path.resolve(rootDir, ref.slice('link:'.length))
}
