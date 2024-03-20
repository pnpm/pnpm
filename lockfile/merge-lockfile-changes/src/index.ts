import '@total-typescript/ts-reset'

import semver from 'semver'
import comverToSemver from 'comver-to-semver'

import type { Lockfile, PackageSnapshot, PackageSnapshots } from '@pnpm/types'

const depsDevDepsOptDeps = [
  'dependencies',
  'devDependencies',
  'optionalDependencies',
] as const

const depsOptDeps = ['dependencies', 'optionalDependencies'] as const

export function mergeLockfileChanges(ours: Lockfile, theirs: Lockfile): Lockfile {
  const newLockfile: Lockfile = {
    importers: {},
    lockfileVersion: semver.gt(
      comverToSemver(theirs.lockfileVersion.toString()),
      comverToSemver(ours.lockfileVersion.toString())
    )
      ? theirs.lockfileVersion
      : ours.lockfileVersion,
  }

  for (const importerId of Array.from(
    new Set([...Object.keys(ours.importers), ...Object.keys(theirs.importers)])
  )) {
    newLockfile.importers[importerId] = {
      specifiers: {},
    }

    for (const key of depsDevDepsOptDeps) {
      newLockfile.importers[importerId][key] = mergeDict(
        ours.importers[importerId]?.[key] ?? {},
        theirs.importers[importerId]?.[key] ?? {},
        mergeVersions
      )

      if (
        Object.keys(newLockfile.importers[importerId][key] ?? {}).length === 0
      ) {
        delete newLockfile.importers[importerId][key]
      }
    }

    newLockfile.importers[importerId].specifiers = mergeDict(
      ours.importers[importerId]?.specifiers ?? {},
      theirs.importers[importerId]?.specifiers ?? {},
      takeChangedValue
    )
  }

  const packages: PackageSnapshots = {}

  for (const depPath of Array.from(
    new Set([
      ...Object.keys(ours.packages ?? {}),
      ...Object.keys(theirs.packages ?? {}),
    ])
  )) {
    const ourPkg = ours.packages?.[depPath]

    const theirPkg = theirs.packages?.[depPath]

    const pkg: PackageSnapshot | undefined = {
      ...ourPkg,
      ...theirPkg,
    }

    for (const key of depsOptDeps) {
      pkg[key] = mergeDict(
        ourPkg?.[key] ?? {},
        theirPkg?.[key] ?? {},
        mergeVersions
      )

      if (Object.keys(pkg[key] ?? {}).length === 0) {
        delete pkg[key]
      }
    }

    packages[depPath] = pkg
  }
  newLockfile.packages = packages

  return newLockfile
}

type ValueMerger<T> = (ourValue: T, theirValue: T) => T

function mergeDict<T>(
  ourDict: Record<string, T>,
  theirDict: Record<string, T>,
  valueMerger: ValueMerger<T>
): Record<string, T> {
  const newDict: Record<string, T> = {}
  for (const key of Object.keys(ourDict).concat(Object.keys(theirDict))) {
    const changedValue = valueMerger(ourDict[key], theirDict[key])
    if (changedValue) {
      newDict[key] = changedValue
    }
  }
  return newDict
}

function takeChangedValue<T>(ourValue: T, theirValue: T): T {
  if (ourValue === theirValue || theirValue == null) return ourValue
  return theirValue
}

function mergeVersions(ourValue: string, theirValue: string): string {
  if (ourValue === theirValue || !theirValue) return ourValue
  if (!ourValue) return theirValue
  const [ourVersion] = ourValue.split('_')
  const [theirVersion] = theirValue.split('_')
  if (semver.gt(ourVersion, theirVersion)) {
    return ourValue
  }
  return theirValue
}
