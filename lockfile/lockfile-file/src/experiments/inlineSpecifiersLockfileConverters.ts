import type { Lockfile, ProjectSnapshot, ResolvedDependencies } from '@pnpm/lockfile-types'
import {
  INLINE_SPECIFIERS_FORMAT_LOCKFILE_VERSION_SUFFIX,
  type InlineSpecifiersLockfile,
  type InlineSpecifiersProjectSnapshot,
  type InlineSpecifiersResolvedDependencies,
} from './InlineSpecifiersLockfile'

export function isExperimentalInlineSpecifiersFormat (
  lockfile: InlineSpecifiersLockfile | Lockfile
): lockfile is InlineSpecifiersLockfile {
  const { lockfileVersion } = lockfile
  return lockfileVersion.toString().startsWith('6.') || typeof lockfileVersion === 'string' && lockfileVersion.endsWith(INLINE_SPECIFIERS_FORMAT_LOCKFILE_VERSION_SUFFIX)
}

export function convertToInlineSpecifiersFormat (lockfile: Lockfile): InlineSpecifiersLockfile {
  const newLockfile = {
    ...lockfile,
    lockfileVersion: lockfile.lockfileVersion.toString().startsWith('6.')
      ? lockfile.lockfileVersion.toString()
      : (
        lockfile.lockfileVersion.toString().endsWith(INLINE_SPECIFIERS_FORMAT_LOCKFILE_VERSION_SUFFIX)
          ? lockfile.lockfileVersion.toString()
          : `${lockfile.lockfileVersion}${INLINE_SPECIFIERS_FORMAT_LOCKFILE_VERSION_SUFFIX}`
      ),
    importers: mapValues(lockfile.importers, convertProjectSnapshotToInlineSpecifiersFormat),
  }
  return newLockfile
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

  const newLockfile = {
    ...rest,
    lockfileVersion: lockfileVersion.endsWith(INLINE_SPECIFIERS_FORMAT_LOCKFILE_VERSION_SUFFIX) ? originalVersion : lockfileVersion,
    importers: mapValues(importers, revertProjectSnapshot),
  }
  return newLockfile
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
  const result: Record<string, U> = {}
  for (const [key, value] of Object.entries(obj)) {
    result[key] = mapper(value, key)
  }
  return result
}
