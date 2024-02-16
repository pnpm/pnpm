import type { Lockfile, ProjectSnapshot, ResolvedDependencies } from '@pnpm/lockfile-types'
import { DEPENDENCIES_FIELDS } from '@pnpm/types'
import { type LockfileFile } from '../write'
import {
  type InlineSpecifiersLockfile,
  type InlineSpecifiersProjectSnapshot,
  type InlineSpecifiersResolvedDependencies,
} from './InlineSpecifiersLockfile'

export function convertToInlineSpecifiersFormat (lockfile: Lockfile): InlineSpecifiersLockfile {
  const newLockfile = {
    ...lockfile,
    lockfileVersion: lockfile.lockfileVersion.toString(),
    importers: mapValues(lockfile.importers, convertProjectSnapshotToInlineSpecifiersFormat),
  }
  return newLockfile
}

/**
 * Reverts changes from the "forceSharedFormat" write option if necessary.
 */
function convertFromLockfileFileMutable (lockfileFile: LockfileFile): InlineSpecifiersLockfile {
  if (typeof lockfileFile?.['importers'] === 'undefined') {
    lockfileFile.importers = {
      '.': {
        dependenciesMeta: lockfileFile['dependenciesMeta'],
        publishDirectory: lockfileFile['publishDirectory'],
      },
    }
    for (const depType of DEPENDENCIES_FIELDS) {
      if (lockfileFile[depType] != null) {
        lockfileFile.importers['.'][depType] = lockfileFile[depType]
        delete lockfileFile[depType]
      }
    }
  }
  return lockfileFile as InlineSpecifiersLockfile
}

export function revertFromInlineSpecifiersFormat (lockfile: LockfileFile): Lockfile {
  const { importers, ...rest } = convertFromLockfileFileMutable(lockfile)

  const newLockfile = {
    ...rest,
    importers: mapValues(importers ?? {}, revertProjectSnapshot),
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
