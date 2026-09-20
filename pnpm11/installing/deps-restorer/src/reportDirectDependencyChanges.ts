import { rootLogger } from '@pnpm/core-loggers'
import * as dp from '@pnpm/deps.path'
import type { LockfileObject, ProjectSnapshot } from '@pnpm/lockfile.fs'
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
 * `link:` dependencies are left out: they are symlinked even under the hoisted
 * linker, so they are already reported.
 */
export function reportDirectDependencyChanges (opts: {
  currentLockfile: LockfileObject | null | undefined
  wantedLockfile: LockfileObject
  projects: Array<{ id: ProjectId, rootDir: ProjectRootDir }>
}): void {
  for (const { id, rootDir } of opts.projects) {
    const before = directDependencies(opts.currentLockfile?.importers[id])
    const after = directDependencies(opts.wantedLockfile.importers[id])
    for (const [alias, dep] of after) {
      const prev = before.get(alias)
      if (prev?.ref === dep.ref) continue
      if (prev != null) {
        reportRemoved(opts.currentLockfile!, alias, prev, rootDir)
      }
      reportAdded(opts.wantedLockfile, alias, dep, rootDir)
    }
    for (const [alias, dep] of before) {
      if (after.has(alias)) continue
      reportRemoved(opts.currentLockfile!, alias, dep, rootDir)
    }
  }
}

function directDependencies (importer: ProjectSnapshot | undefined): Map<string, DirectDependency> {
  const deps = new Map<string, DirectDependency>()
  if (importer == null) return deps
  for (const field of Object.keys(DEPENDENCY_TYPE_BY_FIELD) as DependenciesField[]) {
    for (const [alias, ref] of Object.entries<string>(importer[field] ?? {})) {
      // A `link:` reference resolves to no package in the lockfile, and the
      // linker reports it on its own.
      if (ref.startsWith('link:')) continue
      deps.set(alias, { ref, dependencyType: DEPENDENCY_TYPE_BY_FIELD[field] })
    }
  }
  return deps
}

function reportAdded (
  lockfile: LockfileObject,
  alias: string,
  dep: DirectDependency,
  prefix: ProjectRootDir
): void {
  const pkg = resolvePackage(lockfile, alias, dep.ref)
  if (pkg == null) return
  rootLogger.debug({
    added: {
      dependencyType: dep.dependencyType,
      id: pkg.id,
      name: alias,
      realName: pkg.name,
      version: pkg.version,
    },
    prefix,
  })
}

function reportRemoved (
  lockfile: LockfileObject,
  alias: string,
  dep: DirectDependency,
  prefix: ProjectRootDir
): void {
  const pkg = resolvePackage(lockfile, alias, dep.ref)
  if (pkg == null) return
  rootLogger.debug({
    prefix,
    removed: {
      dependencyType: dep.dependencyType,
      name: alias,
      version: pkg.version,
    },
  })
}

function resolvePackage (
  lockfile: LockfileObject,
  alias: string,
  ref: string
): { id: string, name: string, version: string } | undefined {
  const depPath = dp.refToRelative(ref, alias)
  if (depPath == null) return undefined
  const pkgSnapshot = lockfile.packages?.[depPath]
  if (pkgSnapshot == null) return undefined
  const { name, version } = nameVerFromPkgSnapshot(depPath, pkgSnapshot)
  return { id: pkgSnapshot.id ?? depPath, name, version }
}
