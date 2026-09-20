import { rootLogger } from '@pnpm/core-loggers'
import * as dp from '@pnpm/deps.path'
import type { LockfileObject } from '@pnpm/lockfile.fs'
import { nameVerFromPkgSnapshot } from '@pnpm/lockfile.utils'
import type { DependenciesField, ProjectId, ProjectRootDir } from '@pnpm/types'

type DependencyType = 'prod' | 'dev' | 'optional'

const DEPENDENCY_TYPE_BY_FIELD: Record<DependenciesField, DependencyType> = {
  dependencies: 'prod',
  devDependencies: 'dev',
  optionalDependencies: 'optional',
}

interface DirectDependency {
  ref: string
  dependencyType: DependencyType
  /** Undefined when the lockfile holds no package for the reference. */
  pkg?: { id: string, name: string, version: string }
}

/**
 * Report which of a project's direct dependencies this install added, replaced
 * or dropped. The default reporter turns these into the `+ pkg 1.0.0` summary.
 *
 * Every other linker reports a dependency as it creates its `node_modules`
 * symlink. The hoisted linker writes the package into `node_modules/<alias>`
 * itself and creates no symlink there, so nothing reported it and the reporter
 * fell back to diffing `package.json`, which knows the range a dependency was
 * asked for rather than the version it resolved to (pnpm/pnpm#15161).
 *
 * The symlink outcome answers "did this install put it there". Here the answer
 * comes from the lockfile the previous install left in `node_modules/.pnpm`,
 * which is absent when `node_modules` was deleted, so that install reports
 * everything it puts back.
 *
 * `link:` dependencies are left out: they are symlinked even under the hoisted
 * linker, so they are already reported.
 */
export function reportDirectDependencyChanges (opts: {
  currentLockfile: LockfileObject | null | undefined
  wantedLockfile: LockfileObject
  projects: Array<{ id: ProjectId, rootDir: ProjectRootDir }>
}): void {
  for (const { id, rootDir } of opts.projects) {
    const before = directDependencies(opts.currentLockfile, id)
    const after = directDependencies(opts.wantedLockfile, id)
    for (const [alias, dep] of after) {
      const prev = before.get(alias)
      if (prev?.ref === dep.ref) continue
      if (prev != null) {
        report(alias, prev, rootDir, 'removed')
      }
      report(alias, dep, rootDir, 'added')
    }
    for (const [alias, dep] of before) {
      if (after.has(alias)) continue
      report(alias, dep, rootDir, 'removed')
    }
  }
}

/** The importer's direct dependencies, resolved against its own lockfile. */
function directDependencies (
  lockfile: LockfileObject | null | undefined,
  id: ProjectId
): Map<string, DirectDependency> {
  const deps = new Map<string, DirectDependency>()
  const importer = lockfile?.importers[id]
  if (importer == null || lockfile == null) return deps
  for (const field of Object.keys(DEPENDENCY_TYPE_BY_FIELD) as DependenciesField[]) {
    for (const [alias, ref] of Object.entries<string>(importer[field] ?? {})) {
      if (ref.startsWith('link:') || deps.has(alias)) continue
      deps.set(alias, {
        ref,
        dependencyType: DEPENDENCY_TYPE_BY_FIELD[field],
        pkg: resolvePackage(lockfile, alias, ref),
      })
    }
  }
  return deps
}

function report (
  alias: string,
  dep: DirectDependency,
  prefix: ProjectRootDir,
  action: 'added' | 'removed'
): void {
  if (dep.pkg == null) return
  const { dependencyType } = dep
  const { id, name: realName, version } = dep.pkg
  // `name` is the directory under `node_modules`, which an npm alias makes
  // differ from the package's own name.
  rootLogger.debug(action === 'added'
    ? { added: { dependencyType, id, name: alias, realName, version }, prefix }
    : { prefix, removed: { dependencyType, name: alias, version } }
  )
}

function resolvePackage (
  lockfile: LockfileObject,
  alias: string,
  ref: string
): DirectDependency['pkg'] {
  const depPath = dp.refToRelative(ref, alias)
  if (depPath == null) return undefined
  const pkgSnapshot = lockfile.packages?.[depPath]
  if (pkgSnapshot == null) return undefined
  const { name, version } = nameVerFromPkgSnapshot(depPath, pkgSnapshot)
  return { id: pkgSnapshot.id ?? depPath, name, version }
}
