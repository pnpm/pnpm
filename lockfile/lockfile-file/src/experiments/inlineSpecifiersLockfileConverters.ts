import * as dp from '@pnpm/dependency-path'
import type { Lockfile, ProjectSnapshot, ResolvedDependencies } from '@pnpm/lockfile-types'
import {
  INLINE_SPECIFIERS_FORMAT_LOCKFILE_VERSION_SUFFIX,
  InlineSpecifiersLockfile,
  InlineSpecifiersProjectSnapshot,
  InlineSpecifiersResolvedDependencies,
} from './InlineSpecifiersLockfile'

export function isExperimentalInlineSpecifiersFormat (
  lockfile: InlineSpecifiersLockfile | Lockfile
): lockfile is InlineSpecifiersLockfile {
  const { lockfileVersion } = lockfile
  return lockfileVersion.toString().startsWith('6.') || typeof lockfileVersion === 'string' && lockfileVersion.endsWith(INLINE_SPECIFIERS_FORMAT_LOCKFILE_VERSION_SUFFIX)
}

export function convertToInlineSpecifiersFormat (lockfile: Lockfile): InlineSpecifiersLockfile {
  let importers = lockfile.importers
  let packages = lockfile.packages
  if (lockfile.lockfileVersion.toString().startsWith('6.')) {
    importers = Object.fromEntries(
      Object.entries(lockfile.importers ?? {})
        .map(([importerId, pkgSnapshot]: [string, ProjectSnapshot]) => {
          const newSnapshot = { ...pkgSnapshot }
          if (newSnapshot.dependencies != null) {
            newSnapshot.dependencies = mapValues(newSnapshot.dependencies, convertOldRefToNewRef)
          }
          if (newSnapshot.optionalDependencies != null) {
            newSnapshot.optionalDependencies = mapValues(newSnapshot.optionalDependencies, convertOldRefToNewRef)
          }
          if (newSnapshot.devDependencies != null) {
            newSnapshot.devDependencies = mapValues(newSnapshot.devDependencies, convertOldRefToNewRef)
          }
          return [importerId, newSnapshot]
        })
    )
    packages = Object.fromEntries(
      Object.entries(lockfile.packages ?? {})
        .map(([depPath, pkgSnapshot]) => {
          const newSnapshot = { ...pkgSnapshot }
          if (newSnapshot.dependencies != null) {
            newSnapshot.dependencies = mapValues(newSnapshot.dependencies, convertOldRefToNewRef)
          }
          if (newSnapshot.optionalDependencies != null) {
            newSnapshot.optionalDependencies = mapValues(newSnapshot.optionalDependencies, convertOldRefToNewRef)
          }
          return [convertOldDepPathToNewDepPath(depPath), newSnapshot]
        })
    )
  }
  const newLockfile = {
    ...lockfile,
    packages,
    lockfileVersion: lockfile.lockfileVersion.toString().startsWith('6.')
      ? lockfile.lockfileVersion.toString()
      : (
        lockfile.lockfileVersion.toString().endsWith(INLINE_SPECIFIERS_FORMAT_LOCKFILE_VERSION_SUFFIX)
          ? lockfile.lockfileVersion.toString()
          : `${lockfile.lockfileVersion}${INLINE_SPECIFIERS_FORMAT_LOCKFILE_VERSION_SUFFIX}`
      ),
    importers: mapValues(importers, convertProjectSnapshotToInlineSpecifiersFormat),
  }
  if (lockfile.lockfileVersion.toString().startsWith('6.') && newLockfile.time) {
    newLockfile.time = Object.fromEntries(
      Object.entries(newLockfile.time)
        .map(([depPath, time]) => [convertOldDepPathToNewDepPath(depPath), time])
    )
  }
  return newLockfile
}

function convertOldDepPathToNewDepPath (oldDepPath: string) {
  const parsedDepPath = dp.parse(oldDepPath)
  if (!parsedDepPath.name || !parsedDepPath.version) return oldDepPath
  let newDepPath = `/${parsedDepPath.name}@${parsedDepPath.version}`
  if (parsedDepPath.peersSuffix) {
    if (parsedDepPath.peersSuffix.startsWith('(')) {
      newDepPath += parsedDepPath.peersSuffix
    } else {
      newDepPath += `_${parsedDepPath.peersSuffix}`
    }
  }
  if (parsedDepPath.host) {
    newDepPath = `${parsedDepPath.host}${newDepPath}`
  }
  return newDepPath
}

function convertOldRefToNewRef (oldRef: string) {
  if (oldRef.startsWith('link:') || oldRef.startsWith('file:')) {
    return oldRef
  }
  if (oldRef.includes('/')) {
    return convertOldDepPathToNewDepPath(oldRef)
  }
  return oldRef
}

export function revertFromInlineSpecifiersFormatIfNecessary (lockfile: Lockfile | InlineSpecifiersLockfile): Lockfile {
  return isExperimentalInlineSpecifiersFormat(lockfile)
    ? revertFromInlineSpecifiersFormat(lockfile)
    : lockfile
}

export function revertFromInlineSpecifiersFormat (lockfile: InlineSpecifiersLockfile): Lockfile {
  const { lockfileVersion, importers, ...rest } = lockfile

  const originalVersionStr = lockfileVersion.replace(INLINE_SPECIFIERS_FORMAT_LOCKFILE_VERSION_SUFFIX, '')
  const originalVersion = Number(originalVersionStr)
  if (isNaN(originalVersion)) {
    throw new Error(`Unable to revert lockfile from inline specifiers format. Invalid version parsed: ${originalVersionStr}`)
  }

  let revertedImporters = mapValues(importers, revertProjectSnapshot)
  let packages = lockfile.packages
  if (originalVersion === 6) {
    revertedImporters = Object.fromEntries(
      Object.entries(revertedImporters ?? {})
        .map(([importerId, pkgSnapshot]: [string, ProjectSnapshot]) => {
          const newSnapshot = { ...pkgSnapshot }
          if (newSnapshot.dependencies != null) {
            newSnapshot.dependencies = mapValues(newSnapshot.dependencies, convertNewRefToOldRef)
          }
          if (newSnapshot.optionalDependencies != null) {
            newSnapshot.optionalDependencies = mapValues(newSnapshot.optionalDependencies, convertNewRefToOldRef)
          }
          if (newSnapshot.devDependencies != null) {
            newSnapshot.devDependencies = mapValues(newSnapshot.devDependencies, convertNewRefToOldRef)
          }
          return [importerId, newSnapshot]
        })
    )
    packages = Object.fromEntries(
      Object.entries(lockfile.packages ?? {})
        .map(([depPath, pkgSnapshot]) => {
          const newSnapshot = { ...pkgSnapshot }
          if (newSnapshot.dependencies != null) {
            newSnapshot.dependencies = mapValues(newSnapshot.dependencies, convertNewRefToOldRef)
          }
          if (newSnapshot.optionalDependencies != null) {
            newSnapshot.optionalDependencies = mapValues(newSnapshot.optionalDependencies, convertNewRefToOldRef)
          }
          return [convertNewDepPathToOldDepPath(depPath), newSnapshot]
        })
    )
  }
  const newLockfile = {
    ...rest,
    lockfileVersion: lockfileVersion.endsWith(INLINE_SPECIFIERS_FORMAT_LOCKFILE_VERSION_SUFFIX) ? originalVersion : lockfileVersion,
    packages,
    importers: revertedImporters,
  }
  if (originalVersion === 6 && newLockfile.time) {
    newLockfile.time = Object.fromEntries(
      Object.entries(newLockfile.time)
        .map(([depPath, time]) => [convertNewDepPathToOldDepPath(depPath), time])
    )
  }
  return newLockfile
}

export function convertNewDepPathToOldDepPath (oldDepPath: string) {
  if (!oldDepPath.includes('@', 2)) return oldDepPath
  const index = oldDepPath.indexOf('@', oldDepPath.indexOf('/@') + 2)
  if (oldDepPath.includes('(') && index > oldDepPath.indexOf('(')) return oldDepPath
  return `${oldDepPath.substring(0, index)}/${oldDepPath.substring(index + 1)}`
}

function convertNewRefToOldRef (oldRef: string) {
  if (oldRef.startsWith('link:') || oldRef.startsWith('file:')) {
    return oldRef
  }
  if (oldRef.includes('@')) {
    return convertNewDepPathToOldDepPath(oldRef)
  }
  return oldRef
}

function convertProjectSnapshotToInlineSpecifiersFormat (
  projectSnapshot: ProjectSnapshot
): InlineSpecifiersProjectSnapshot {
  const { specifiers, ...rest } = projectSnapshot
  const convertBlock = (block?: ResolvedDependencies) =>
    block != null
      ? convertResolvedDependenciesToInlineSpecifiersFormat(block, { specifiers })
      : block
  return {
    ...rest,
    dependencies: convertBlock(projectSnapshot.dependencies),
    optionalDependencies: convertBlock(projectSnapshot.optionalDependencies),
    devDependencies: convertBlock(projectSnapshot.devDependencies),
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
  const result = {}
  for (const [key, value] of Object.entries(obj)) {
    result[key] = mapper(value, key)
  }
  return result
}
