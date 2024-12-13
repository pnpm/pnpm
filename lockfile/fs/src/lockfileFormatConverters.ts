import { parseDepPath, removeSuffix } from '@pnpm/dependency-path'
import { createGitHostedPkgId } from '@pnpm/git-resolver'
import {
  type LockfileObject,
  type ProjectSnapshot,
  type LockfilePackageSnapshot,
  type ResolvedDependencies,
  type LockfileFile,
  type InlineSpecifiersLockfile,
  type InlineSpecifiersProjectSnapshot,
  type InlineSpecifiersResolvedDependencies,
  type LockfilePackageInfo,
  type LockfileFileV9,
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

function normalizeLockfile (lockfile: InlineSpecifiersLockfile): LockfileFile {
  const lockfileToSave: LockfileFile = {
    ...lockfile,
    importers: _mapValues((importer) => {
      const normalizedImporter: Partial<InlineSpecifiersProjectSnapshot> = {}
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
      return normalizedImporter as InlineSpecifiersProjectSnapshot
    }, lockfile.importers ?? {}),
  }
  if (isEmpty(lockfileToSave.packages) || (lockfileToSave.packages == null)) {
    delete lockfileToSave.packages
  }
  if (isEmpty((lockfileToSave as LockfileFileV9).snapshots) || ((lockfileToSave as LockfileFileV9).snapshots == null)) {
    delete (lockfileToSave as LockfileFileV9).snapshots
  }
  if (lockfileToSave.time) {
    lockfileToSave.time = pruneTimeInLockfileV6(lockfileToSave.time, lockfile.importers ?? {})
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

function pruneTimeInLockfileV6 (time: Record<string, string>, importers: Record<string, InlineSpecifiersProjectSnapshot>): Record<string, string> {
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

export function convertToLockfileObject (lockfile: LockfileFile | LockfileFileV9): LockfileObject {
  if ((lockfile as LockfileFileV9).snapshots) {
    return convertLockfileV9ToLockfileObject(lockfile as LockfileFileV9)
  }
  convertPkgIds(lockfile)
  const { importers, ...rest } = lockfile

  const newLockfile = {
    ...rest,
    importers: mapValues(importers ?? {}, revertProjectSnapshot),
  }
  return newLockfile
}

function convertPkgIds (lockfile: LockfileFile): void {
  const oldIdToNewId: Record<string, DepPath> = {}
  if (lockfile.packages == null || isEmpty(lockfile.packages)) return
  for (const [pkgId, pkg] of Object.entries(lockfile.packages ?? {})) {
    if (pkg.name) {
      const { id, peersSuffix } = parseDepPath(pkgId)
      let newId = `${pkg.name}@`
      if ('tarball' in pkg.resolution) {
        newId += pkg.resolution.tarball
        if (pkg.resolution.path) {
          newId += `#path:${pkg.resolution.path}`
        }
      } else if ('repo' in pkg.resolution) {
        newId += createGitHostedPkgId(pkg.resolution)
      } else if ('directory' in pkg.resolution) {
        newId += id
      } else {
        continue
      }
      oldIdToNewId[pkgId] = `${newId}${peersSuffix}` as DepPath
      if (id !== pkgId) {
        oldIdToNewId[id] = newId as DepPath
      }
    } else {
      const { id, peersSuffix } = parseDepPath(pkgId)
      const newId = id.substring(1)
      oldIdToNewId[pkgId] = `${newId}${peersSuffix}` as DepPath
      if (id !== pkgId) {
        oldIdToNewId[id] = newId as DepPath
      }
    }
  }
  const newLockfilePackages: PackageSnapshots = {}
  for (const [pkgId, pkg] of Object.entries(lockfile.packages ?? {})) {
    if (oldIdToNewId[pkgId]) {
      if (pkg.id) {
        pkg.id = oldIdToNewId[pkg.id]
      }
      newLockfilePackages[oldIdToNewId[pkgId]] = pkg
    } else {
      newLockfilePackages[pkgId as DepPath] = pkg
    }
    for (const depType of ['dependencies', 'optionalDependencies'] as const) {
      for (const [alias, depPath] of Object.entries(pkg[depType] ?? {})) {
        if (oldIdToNewId[depPath as string]) {
          if (oldIdToNewId[depPath as string].startsWith(`${alias}@`)) {
            pkg[depType]![alias] = oldIdToNewId[depPath as string].substring(alias.length + 1)
          } else {
            pkg[depType]![alias] = oldIdToNewId[depPath as string]
          }
        }
      }
    }
  }
  lockfile.packages = newLockfilePackages
  for (const importer of Object.values(lockfile.importers ?? {})) {
    for (const depType of ['dependencies', 'optionalDependencies', 'devDependencies'] as const) {
      for (const [alias, { version }] of Object.entries(importer[depType] ?? {})) {
        if (oldIdToNewId[version]) {
          if (oldIdToNewId[version].startsWith(`${alias}@`)) {
            importer[depType]![alias].version = oldIdToNewId[version].substring(alias.length + 1)
          } else {
            importer[depType]![alias].version = oldIdToNewId[version]
          }
        }
      }
    }
  }
}

export function convertLockfileV9ToLockfileObject (lockfile: LockfileFileV9): LockfileObject {
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
): InlineSpecifiersProjectSnapshot {
  const { specifiers, ...rest } = projectSnapshot
  if (specifiers == null) return projectSnapshot as InlineSpecifiersProjectSnapshot
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
): InlineSpecifiersResolvedDependencies {
  return mapValues(resolvedDependencies, (version, depName) => ({
    specifier: specifiers[depName],
    version,
  }))
}

function revertProjectSnapshot (from: InlineSpecifiersProjectSnapshot): ProjectSnapshot {
  const specifiers: ResolvedDependencies = {}

  function moveSpecifiers (from: InlineSpecifiersResolvedDependencies): ResolvedDependencies {
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
