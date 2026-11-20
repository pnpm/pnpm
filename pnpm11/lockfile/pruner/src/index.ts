import { LOCKFILE_VERSION } from '@pnpm/constants'
import { refToRelative } from '@pnpm/deps.path'
import type {
  LockfileObject,
  PackageSnapshot,
  PackageSnapshots,
  ProjectSnapshot,
  ResolvedDependencies,
} from '@pnpm/lockfile.types'
import type { DepPath, PackageManifest, ProjectId } from '@pnpm/types'
import { difference, isEmpty, unnest } from 'ramda'

export * from '@pnpm/lockfile.types'

// cannot import DependenciesGraph from @pnpm/installing.deps-resolver due to circular dependency
type DependenciesGraph = Record<DepPath, { optional?: boolean }>

export function pruneSharedLockfile (
  lockfile: LockfileObject,
  opts?: {
    dependenciesGraph?: DependenciesGraph
    warn?: (msg: string) => void
  }
): LockfileObject {
  const copiedPackages = (lockfile.packages == null)
    ? {}
    : copyPackageSnapshots(lockfile.packages, {
      devDepPaths: unnest(Object.values(lockfile.importers).map((deps) => resolvedDepsToDepPaths(deps.devDependencies ?? {}))),
      optionalDepPaths: unnest(Object.values(lockfile.importers).map((deps) => resolvedDepsToDepPaths(deps.optionalDependencies ?? {}))),
      prodDepPaths: unnest(Object.values(lockfile.importers).map((deps) => resolvedDepsToDepPaths(deps.dependencies ?? {}))),
      warn: opts?.warn ?? ((_msg: string) => undefined),
      dependenciesGraph: opts?.dependenciesGraph,
    })

  const prunedLockfile: LockfileObject = {
    ...lockfile,
    packages: copiedPackages,
  }
  if (isEmpty(prunedLockfile.packages)) {
    delete prunedLockfile.packages
  }
  return prunedLockfile
}

interface PrunedImporterDependencies {
  specifiers: ResolvedDependencies
  dependencies: ResolvedDependencies
  optionalDependencies: ResolvedDependencies
  devDependencies: ResolvedDependencies
}

export function pruneLockfile (
  lockfile: LockfileObject,
  pkg: PackageManifest,
  importerId: ProjectId,
  opts: {
    warn?: (msg: string) => void
    dependenciesGraph?: DependenciesGraph
  }
): LockfileObject {
  const importer = lockfile.importers[importerId]
  const manifestDeps = extractManifestDeps(pkg)
  const prunedDeps = pruneImporterDependencies(importer, manifestDeps)
  const updatedImporter: ProjectSnapshot = buildUpdatedImporter(prunedDeps)
  const prunedLockfile = buildPrunedLockfile(lockfile, importerId, updatedImporter)
  return pruneSharedLockfile(prunedLockfile, opts)
}

function extractManifestDeps (pkg: PackageManifest): Set<string> {
  const optionalDependencies = Object.keys(pkg.optionalDependencies ?? {})
  const dependencies = difference(Object.keys(pkg.dependencies ?? {}), optionalDependencies)
  const devDependencies = difference(difference(Object.keys(pkg.devDependencies ?? {}), optionalDependencies), dependencies)
  return new Set([
    ...optionalDependencies,
    ...devDependencies,
    ...dependencies,
  ])
}

function pruneImporterDependencies (
  importer: ProjectSnapshot,
  allDeps: Set<string>
): PrunedImporterDependencies {
  const specifiers: ResolvedDependencies = {}
  const dependencies: ResolvedDependencies = {}
  const optionalDependencies: ResolvedDependencies = {}
  const devDependencies: ResolvedDependencies = {}

  for (const depName in importer.specifiers ?? {}) {
    if (!allDeps.has(depName)) continue
    specifiers[depName] = importer.specifiers![depName]
    if (importer.dependencies?.[depName]) {
      dependencies[depName] = importer.dependencies[depName]
    } else if (importer.optionalDependencies?.[depName]) {
      optionalDependencies[depName] = importer.optionalDependencies[depName]
    } else if (importer.devDependencies?.[depName]) {
      devDependencies[depName] = importer.devDependencies[depName]
    }
  }
  preserveLinkedDependencies(importer, allDeps, dependencies)
  return { specifiers, dependencies, optionalDependencies, devDependencies }
}

function preserveLinkedDependencies (
  importer: ProjectSnapshot,
  allDeps: Set<string>,
  targetDependencies: ResolvedDependencies
): void {
  if (importer.dependencies == null) return
  for (const [alias, dep] of Object.entries(importer.dependencies)) {
    const isUnlistedLink = !targetDependencies[alias] && dep.startsWith('link:')
    const isRemovedFromPkgJson = importer.specifiers?.[alias] != null && !allDeps.has(alias)
    if (isUnlistedLink && !isRemovedFromPkgJson) {
      targetDependencies[alias] = dep
    }
  }
}

function buildUpdatedImporter (prunedDeps: PrunedImporterDependencies): ProjectSnapshot {
  const updated: ProjectSnapshot = {
    specifiers: prunedDeps.specifiers,
  }
  if (!isEmpty(prunedDeps.dependencies)) {
    updated.dependencies = prunedDeps.dependencies
  }
  if (!isEmpty(prunedDeps.optionalDependencies)) {
    updated.optionalDependencies = prunedDeps.optionalDependencies
  }
  if (!isEmpty(prunedDeps.devDependencies)) {
    updated.devDependencies = prunedDeps.devDependencies
  }
  return updated
}

function buildPrunedLockfile (
  lockfile: LockfileObject,
  importerId: ProjectId,
  updatedImporter: ProjectSnapshot
): LockfileObject {
  const pruned: LockfileObject = {
    importers: {
      ...lockfile.importers,
      [importerId]: updatedImporter,
    },
    lockfileVersion: lockfile.lockfileVersion || LOCKFILE_VERSION,
    packages: lockfile.packages,
  }
  if (lockfile.pnpmfileChecksum) {
    pruned.pnpmfileChecksum = lockfile.pnpmfileChecksum
  }
  if (lockfile.untrackedPnpmfileReadPackageHook != null) {
    pruned.untrackedPnpmfileReadPackageHook = lockfile.untrackedPnpmfileReadPackageHook
  }
  if (lockfile.ignoredOptionalDependencies && !isEmpty(lockfile.ignoredOptionalDependencies)) {
    pruned.ignoredOptionalDependencies = lockfile.ignoredOptionalDependencies
  }
  return pruned
}


function copyPackageSnapshots (
  originalPackages: PackageSnapshots,
  opts: {
    devDepPaths: DepPath[]
    optionalDepPaths: DepPath[]
    prodDepPaths: DepPath[]
    warn: (msg: string) => void
    dependenciesGraph?: DependenciesGraph
  }
): PackageSnapshots {
  const copiedSnapshots: PackageSnapshots = {}
  const ctx = {
    copiedSnapshots,
    nonOptional: new Set<string>(),
    originalPackages,
    walked: new Set<string>(),
    warn: opts.warn,
    dependenciesGraph: opts.dependenciesGraph,
  }

  copyDependencySubGraph(ctx, opts.devDepPaths, {
    optional: false,
  })
  copyDependencySubGraph(ctx, opts.optionalDepPaths, {
    optional: true,
  })
  copyDependencySubGraph(ctx, opts.prodDepPaths, {
    optional: false,
  })

  return copiedSnapshots
}

function resolvedDepsToDepPaths (deps: ResolvedDependencies): DepPath[] {
  return Object.entries(deps)
    .map(([alias, ref]) => refToRelative(ref, alias))
    .filter((depPath) => depPath !== null) as DepPath[]
}

interface CopyContext {
  copiedSnapshots: PackageSnapshots
  nonOptional: Set<string>
  originalPackages: PackageSnapshots
  walked: Set<string>
  warn: (msg: string) => void
  dependenciesGraph?: DependenciesGraph
}

function copyDependencySubGraph (
  ctx: CopyContext,
  depPaths: DepPath[],
  opts: {
    optional: boolean
  }
): void {
  for (const depPath of depPaths) {
    const key = `${depPath}:${opts.optional.toString()}`
    if (ctx.walked.has(key)) continue
    ctx.walked.add(key)
    const depLockfile = ctx.originalPackages[depPath]
    if (!depLockfile) {
      if (skipUnresolvableLocalDep(depPath)) continue
      ctx.warn(`Cannot find resolution of ${depPath} in lockfile`)
      continue
    }
    ctx.copiedSnapshots[depPath] = depLockfile
    updateSnapshotOptionalFlag(ctx, depPath, depLockfile, opts.optional)
    const newDependencies = resolvedDepsToDepPaths(depLockfile.dependencies ?? {})
    copyDependencySubGraph(ctx, newDependencies, opts)
    const newOptionalDependencies = resolvedDepsToDepPaths(depLockfile.optionalDependencies ?? {})
    copyDependencySubGraph(ctx, newOptionalDependencies, { optional: true })
  }
}

function skipUnresolvableLocalDep (depPath: DepPath): boolean {
  return depPath.startsWith('link:') || (depPath.startsWith('file:') && !depPath.endsWith('.tar.gz'))
}

function updateSnapshotOptionalFlag (
  ctx: CopyContext,
  depPath: DepPath,
  depLockfile: PackageSnapshot,
  optional: boolean
): void {
  if (optional && !ctx.nonOptional.has(depPath)) {
    depLockfile.optional = true
    if (ctx.dependenciesGraph?.[depPath]) {
      ctx.dependenciesGraph[depPath].optional = true
    }
  } else {
    ctx.nonOptional.add(depPath)
    delete depLockfile.optional
    if (ctx.dependenciesGraph?.[depPath]) {
      ctx.dependenciesGraph[depPath].optional = false
    }
  }
}

