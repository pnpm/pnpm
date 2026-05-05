import { LOCKFILE_VERSION } from '@pnpm/constants'
import { refToRelative } from '@pnpm/deps.path'
import type {
  LockfileObject,
  PackageSnapshots,
  ProjectSnapshot,
  ResolvedDependencies,
  SpecifierAndResolution,
} from '@pnpm/lockfile.types'
import type { DepPath, PackageManifest, ProjectId } from '@pnpm/types'
import { difference, isEmpty, unnest } from 'ramda'

export * from '@pnpm/lockfile.types'

// cannot import DependenciesGraph from @pnpm/installing.deps-resolver due to circular dependency
type DependenciesGraph = Record<DepPath, { optional?: boolean }>
type DependencyReference = string | SpecifierAndResolution
type ResolvedDependenciesWithInlineSpecifiers = Record<string, DependencyReference>

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
  const lockfileSpecs: ResolvedDependencies = importer.specifiers ?? {}
  const optionalDependencies = Object.keys(pkg.optionalDependencies ?? {})
  const dependencies = difference(Object.keys(pkg.dependencies ?? {}), optionalDependencies)
  const devDependencies = difference(difference(Object.keys(pkg.devDependencies ?? {}), optionalDependencies), dependencies)
  const allDeps = new Set([
    ...optionalDependencies,
    ...devDependencies,
    ...dependencies,
  ])
  const specifiers: ResolvedDependencies = {}
  const lockfileDependencies: ResolvedDependencies = {}
  const lockfileOptionalDependencies: ResolvedDependencies = {}
  const lockfileDevDependencies: ResolvedDependencies = {}

  const lockfileDepNames = new Set([
    ...Object.keys(lockfileSpecs),
    ...Object.keys(importer.dependencies ?? {}),
    ...Object.keys(importer.optionalDependencies ?? {}),
    ...Object.keys(importer.devDependencies ?? {}),
  ])
  for (const depName of lockfileDepNames) {
    if (!allDeps.has(depName)) continue
    const specifier = lockfileSpecs[depName] ?? getDependencySpecifier(importer.dependencies?.[depName])
      ?? getDependencySpecifier(importer.optionalDependencies?.[depName])
      ?? getDependencySpecifier(importer.devDependencies?.[depName])
    if (specifier != null) {
      specifiers[depName] = specifier
    }
    if (importer.dependencies?.[depName]) {
      lockfileDependencies[depName] = getDependencyReference(importer.dependencies[depName])
    } else if (importer.optionalDependencies?.[depName]) {
      lockfileOptionalDependencies[depName] = getDependencyReference(importer.optionalDependencies[depName])
    } else if (importer.devDependencies?.[depName]) {
      lockfileDevDependencies[depName] = getDependencyReference(importer.devDependencies[depName])
    }
  }
  if (importer.dependencies != null) {
    for (const [alias, dep] of Object.entries(importer.dependencies) as Array<[string, DependencyReference]>) {
      const reference = getDependencyReference(dep)
      const specifier = lockfileSpecs[alias] ?? getDependencySpecifier(dep)
      if (
        !lockfileDependencies[alias] && reference.startsWith('link:') &&
        // If the linked dependency was removed from package.json
        // then it is removed from pnpm-lock.yaml as well
        !(specifier && !allDeps.has(alias))
      ) {
        lockfileDependencies[alias] = reference
      }
    }
  }

  const updatedImporter: ProjectSnapshot = {
    specifiers,
  }
  const prunedLockfile: LockfileObject = {
    importers: {
      ...lockfile.importers,
      [importerId]: updatedImporter,
    },
    lockfileVersion: lockfile.lockfileVersion || LOCKFILE_VERSION,
    packages: lockfile.packages,
  }
  if (!isEmpty(lockfileDependencies)) {
    updatedImporter.dependencies = lockfileDependencies
  }
  if (!isEmpty(lockfileOptionalDependencies)) {
    updatedImporter.optionalDependencies = lockfileOptionalDependencies
  }
  if (!isEmpty(lockfileDevDependencies)) {
    updatedImporter.devDependencies = lockfileDevDependencies
  }
  if (lockfile.pnpmfileChecksum) {
    prunedLockfile.pnpmfileChecksum = lockfile.pnpmfileChecksum
  }
  if (lockfile.ignoredOptionalDependencies && !isEmpty(lockfile.ignoredOptionalDependencies)) {
    prunedLockfile.ignoredOptionalDependencies = lockfile.ignoredOptionalDependencies
  }
  return pruneSharedLockfile(prunedLockfile, opts)
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

function resolvedDepsToDepPaths (deps: ResolvedDependenciesWithInlineSpecifiers): DepPath[] {
  return Object.entries(deps)
    .map(([alias, ref]) => refToRelative(getDependencyReference(ref), alias))
    .filter((depPath) => depPath !== null) as DepPath[]
}

function getDependencyReference (dependency: DependencyReference): string {
  return typeof dependency === 'string' ? dependency : dependency.version
}

function getDependencySpecifier (dependency: DependencyReference | undefined): string | undefined {
  return typeof dependency === 'string' ? undefined : dependency?.specifier
}

function copyDependencySubGraph (
  ctx: {
    copiedSnapshots: PackageSnapshots
    nonOptional: Set<string>
    originalPackages: PackageSnapshots
    walked: Set<string>
    warn: (msg: string) => void
    dependenciesGraph?: DependenciesGraph
  },
  depPaths: DepPath[],
  opts: {
    optional: boolean
  }
): void {
  for (const depPath of depPaths) {
    const key = `${depPath}:${opts.optional.toString()}`
    if (ctx.walked.has(key)) continue
    ctx.walked.add(key)
    if (!ctx.originalPackages[depPath]) {
      // local dependencies don't need to be resolved in pnpm-lock.yaml
      // except local tarball dependencies
      if (depPath.startsWith('link:') || depPath.startsWith('file:') && !depPath.endsWith('.tar.gz')) continue

      ctx.warn(`Cannot find resolution of ${depPath} in lockfile`)
      continue
    }
    const depLockfile = ctx.originalPackages[depPath]
    ctx.copiedSnapshots[depPath] = depLockfile
    if (opts.optional && !ctx.nonOptional.has(depPath)) {
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
    const newDependencies = resolvedDepsToDepPaths(depLockfile.dependencies ?? {})
    copyDependencySubGraph(ctx, newDependencies, opts)
    const newOptionalDependencies = resolvedDepsToDepPaths(depLockfile.optionalDependencies ?? {})
    copyDependencySubGraph(ctx, newOptionalDependencies, { optional: true })
  }
}
