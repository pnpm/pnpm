import path from 'node:path'

import {
  getLockfileImporterId,
  type LockfileObject,
  type ProjectSnapshot,
  type ResolvedDependencies,
} from '@pnpm/lockfile.fs'
import type { IncludedDependencies } from '@pnpm/types'

/**
 * Whether `dedupeDirectDeps` links nothing into the project at `projectDir`:
 * for every alias the project declares in a materialized group, the wanted
 * lockfile records one target on each side, and the two are the same, which
 * is what the linker compares. An alias declared with differing targets in
 * several groups has one effective target the linker picks by group order;
 * that choice is not reproduced here, so such an alias proves nothing.
 * Neither does a lockfile that lacks either importer.
 */
export function dedupeLinksNothing (
  lockfile: LockfileObject,
  lockfileDir: string,
  rootDir: string,
  projectDir: string,
  include?: IncludedDependencies
): boolean {
  const root = lockfile.importers[getLockfileImporterId(lockfileDir, rootDir)]
  const project = lockfile.importers[getLockfileImporterId(lockfileDir, projectDir)]
  if (root == null || project == null) return false
  const materializedGroups = (importer: ProjectSnapshot): Array<ResolvedDependencies | undefined> => [
    include?.dependencies === false ? undefined : importer.dependencies,
    include?.devDependencies === false ? undefined : importer.devDependencies,
    include?.optionalDependencies === false ? undefined : importer.optionalDependencies,
  ]
  const soleTarget = (importer: ProjectSnapshot, alias: string): string | undefined => {
    const versions = materializedGroups(importer)
      .map((deps) => deps?.[alias])
      .filter((version): version is string => version != null)
    return versions.length > 0 && versions.every((version) => version === versions[0]) ? versions[0] : undefined
  }
  const aliases = new Set(materializedGroups(project).flatMap((deps) => Object.keys(deps ?? {})))
  return [...aliases].every((alias) => {
    const version = soleTarget(project, alias)
    const rootVersion = soleTarget(root, alias)
    return version != null && rootVersion != null && resolvesToSameTarget(rootDir, rootVersion, projectDir, version)
  })
}

/**
 * Whether two importer dependency versions resolve to one target: the same
 * snapshot, or `link:` paths that name the same directory once resolved
 * against their own importer directories.
 */
function resolvesToSameTarget (rootDir: string, rootVersion: string, projectDir: string, version: string): boolean {
  const rootLink = rootVersion.startsWith('link:')
  const projectLink = version.startsWith('link:')
  if (rootLink !== projectLink) return false
  if (!rootLink) return rootVersion === version
  return path.resolve(rootDir, rootVersion.slice('link:'.length)) === path.resolve(projectDir, version.slice('link:'.length))
}
