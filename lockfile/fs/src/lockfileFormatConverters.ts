import { removeSuffix } from '@pnpm/dependency-path'
import {
  type LockfileObject,
  type ProjectSnapshot,
  type LockfilePackageSnapshot,
  type ResolvedDependencies,
  type LockfileFile,
  type LockfileFileProjectSnapshot,
  type LockfileFileProjectResolvedDependencies,
  type LockfilePackageInfo,
  type PackageSnapshots,
} from '@pnpm/lockfile.types'
import { type DepPath, DEPENDENCIES_FIELDS } from '@pnpm/types'
import isEmpty from 'ramda/src/isEmpty'
import _mapValues from 'ramda/src/map'
import omit from 'ramda/src/omit'
import pickBy from 'ramda/src/pickBy'
import pick from 'ramda/src/pick'
import { LOCKFILE_VERSION } from '@pnpm/constants'

export function convertToLockfileFile (lockfile: LockfileObject): LockfileFile {
  const packages: Record<string, LockfilePackageInfo> = {}
  const snapshots: Record<string, LockfilePackageSnapshot> = {}
  for (const [depPath, pkg] of Object.entries(lockfile.packages ?? {})) {
    snapshots[depPath] = pick([
      'dependencies',
      'optionalDependencies',
      'transitivePeerDependencies',
      'optional',
      'id',
    ], pkg)
    const pkgId = removeSuffix(depPath)
    if (!packages[pkgId]) {
      packages[pkgId] = pick([
        'bundledDependencies',
        'cpu',
        'deprecated',
        'engines',
        'hasBin',
        'libc',
        'name',
        'os',
        'peerDependencies',
        'peerDependenciesMeta',
        'resolution',
        'version',
      ], pkg)
    }
  }
  const newLockfile = {
    ...lockfile,
    snapshots,
    packages,
    lockfileVersion: LOCKFILE_VERSION,
    importers: mapValues(lockfile.importers, convertProjectSnapshotToInlineSpecifiersFormat),
  }
  if (newLockfile.settings?.peersSuffixMaxLength === 1000) {
    newLockfile.settings = omit(['peersSuffixMaxLength'], newLockfile.settings)
  }
  if (newLockfile.settings?.injectWorkspacePackages === false) {
    delete newLockfile.settings.injectWorkspacePackages
  }
  return normalizeLockfile(newLockfile)
}

function normalizeLockfile (lockfile: LockfileFile): LockfileFile {
  const lockfileToSave = {
    ...lockfile,
    importers: _mapValues((importer) => {
      const normalizedImporter: Partial<LockfileFileProjectSnapshot> = {}
      if (importer.dependenciesMeta != null && !isEmpty(importer.dependenciesMeta)) {
        normalizedImporter.dependenciesMeta = importer.dependenciesMeta
      }
      for (const depType of DEPENDENCIES_FIELDS) {
        if (!isEmpty(importer[depType] ?? {})) {
          normalizedImporter[depType] = importer[depType]
        }
      }
      if (importer.publishDirectory) {
        normalizedImporter.publishDirectory = importer.publishDirectory
      }
      return normalizedImporter as LockfileFileProjectSnapshot
    }, lockfile.importers ?? {}),
  }
  if (isEmpty(lockfileToSave.packages) || (lockfileToSave.packages == null)) {
    delete lockfileToSave.packages
  }
  if (isEmpty((lockfileToSave as LockfileFile).snapshots) || ((lockfileToSave as LockfileFile).snapshots == null)) {
    delete (lockfileToSave as LockfileFile).snapshots
  }
  if (lockfileToSave.time) {
    lockfileToSave.time = pruneTimeInLockfile(lockfileToSave.time, lockfile.importers ?? {})
  }
  if ((lockfileToSave.catalogs != null) && isEmpty(lockfileToSave.catalogs)) {
    delete lockfileToSave.catalogs
  }
  if ((lockfileToSave.overrides != null) && isEmpty(lockfileToSave.overrides)) {
    delete lockfileToSave.overrides
  }
  if ((lockfileToSave.patchedDependencies != null) && isEmpty(lockfileToSave.patchedDependencies)) {
    delete lockfileToSave.patchedDependencies
  }
  if (!lockfileToSave.packageExtensionsChecksum) {
    delete lockfileToSave.packageExtensionsChecksum
  }
  if (!lockfileToSave.ignoredOptionalDependencies?.length) {
    delete lockfileToSave.ignoredOptionalDependencies
  }
  if (!lockfileToSave.pnpmfileChecksum) {
    delete lockfileToSave.pnpmfileChecksum
  }
  return lockfileToSave
}

function pruneTimeInLockfile (time: Record<string, string>, importers: Record<string, LockfileFileProjectSnapshot>): Record<string, string> {
  const rootDepPaths = new Set<string>()
  for (const importer of Object.values(importers)) {
    for (const depType of DEPENDENCIES_FIELDS) {
      for (const [depName, ref] of Object.entries(importer[depType] ?? {})) {
        const suffixStart = ref.version.indexOf('(')
        const refWithoutPeerSuffix = suffixStart === -1 ? ref.version : ref.version.slice(0, suffixStart)
        const depPath = refToRelative(refWithoutPeerSuffix, depName)
        if (!depPath) continue
        rootDepPaths.add(depPath)
      }
    }
  }
  return pickBy((_, depPath) => rootDepPaths.has(depPath), time)
}

function refToRelative (
  reference: string,
  pkgName: string
): string | null {
  if (reference.startsWith('link:')) {
    return null
  }
  if (reference.startsWith('file:')) {
    return reference
  }
  if (!reference.includes('/') || !reference.replace(/(\([^)]+\))+$/, '').includes('/')) {
    return `/${pkgName}@${reference}`
  }
  return reference
}

export function convertToLockfileObject (lockfile: LockfileFile): LockfileObject {
  const { importers, ...rest } = lockfile

  const packages: PackageSnapshots = {}
  for (const [depPath, pkg] of Object.entries(lockfile.snapshots ?? {})) {
    const pkgId = removeSuffix(depPath)
    packages[depPath as DepPath] = Object.assign(pkg, lockfile.packages?.[pkgId])
  }
  return {
    ...omit(['snapshots'], rest),
    packages,
    importers: mapValues(importers ?? {}, revertProjectSnapshot),
  }
}

function convertProjectSnapshotToInlineSpecifiersFormat (
  projectSnapshot: ProjectSnapshot
): LockfileFileProjectSnapshot {
  const { specifiers, ...rest } = projectSnapshot
  if (specifiers == null) return projectSnapshot as LockfileFileProjectSnapshot
  const convertBlock = (block?: ResolvedDependencies) =>
    block != null
      ? convertResolvedDependenciesToInlineSpecifiersFormat(block, { specifiers })
      : block
  return {
    ...rest,
    dependencies: convertBlock(projectSnapshot.dependencies ?? {}),
    optionalDependencies: convertBlock(projectSnapshot.optionalDependencies ?? {}),
    devDependencies: convertBlock(projectSnapshot.devDependencies ?? {}),
  }
}

function convertResolvedDependenciesToInlineSpecifiersFormat (
  resolvedDependencies: ResolvedDependencies,
  { specifiers }: { specifiers: ResolvedDependencies }
): LockfileFileProjectResolvedDependencies {
  return mapValues(resolvedDependencies, (version, depName) => ({
    specifier: specifiers[depName],
    version,
  }))
}

function revertProjectSnapshot (from: LockfileFileProjectSnapshot): ProjectSnapshot {
  const specifiers: ResolvedDependencies = {}

  function moveSpecifiers (from: LockfileFileProjectResolvedDependencies): ResolvedDependencies {
    const resolvedDependencies: ResolvedDependencies = {}
    for (const [depName, { specifier, version }] of Object.entries(from)) {
      const existingValue = specifiers[depName]
      if (existingValue != null && existingValue !== specifier) {
        throw new Error(`Project snapshot lists the same dependency more than once with conflicting versions: ${depName}`)
      }

      specifiers[depName] = specifier
      resolvedDependencies[depName] = version
    }
    return resolvedDependencies
  }

  const dependencies = from.dependencies == null
    ? from.dependencies
    : moveSpecifiers(from.dependencies)
  const devDependencies = from.devDependencies == null
    ? from.devDependencies
    : moveSpecifiers(from.devDependencies)
  const optionalDependencies = from.optionalDependencies == null
    ? from.optionalDependencies
    : moveSpecifiers(from.optionalDependencies)

  return {
    ...from,
    specifiers,
    dependencies,
    devDependencies,
    optionalDependencies,
  }
}

function mapValues<T, U> (obj: Record<string, T>, mapper: (val: T, key: string) => U): Record<string, U> {
  const result: Record<string, U> = {}
  for (const [key, value] of Object.entries(obj)) {
    result[key] = mapper(value, key)
  }
  return result
}
