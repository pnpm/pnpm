import {
  type Lockfile,
  type PackageSnapshot,
  type PackageSnapshots,
  type VersionAndSpecifier,
} from '@pnpm/lockfile-types'
import comverToSemver from 'comver-to-semver'
import semver from 'semver'

export function mergeLockfileChanges (ours: Lockfile, theirs: Lockfile) {
  const newLockfile: Lockfile = {
    importers: {},
    lockfileVersion: semver.gt(comverToSemver(theirs.lockfileVersion.toString()), comverToSemver(ours.lockfileVersion.toString()))
      ? theirs.lockfileVersion
      : ours.lockfileVersion,
  }

  for (const importerId of Array.from(new Set([...Object.keys(ours.importers), ...Object.keys(theirs.importers)]))) {
    newLockfile.importers[importerId] = {
    }
    for (const key of ['dependencies', 'devDependencies', 'optionalDependencies'] as const) {
      newLockfile.importers[importerId][key] = mergeDict(
        ours.importers[importerId]?.[key] ?? {},
        theirs.importers[importerId]?.[key] ?? {},
        mergeImporterVersions
      )
      if (Object.keys(newLockfile.importers[importerId][key] ?? {}).length === 0) {
        delete newLockfile.importers[importerId][key]
      }
    }
  }

  const packages: PackageSnapshots = {}
  for (const depPath of Array.from(new Set([...Object.keys(ours.packages ?? {}), ...Object.keys(theirs.packages ?? {})]))) {
    const ourPkg = ours.packages?.[depPath]
    const theirPkg = theirs.packages?.[depPath]
    const pkg = {
      ...ourPkg,
      ...theirPkg,
    }
    for (const key of ['dependencies', 'optionalDependencies'] as const) {
      pkg[key] = mergeDict(
        ourPkg?.[key] ?? {},
        theirPkg?.[key] ?? {},
        mergeVersions
      )
      if (Object.keys(pkg[key] ?? {}).length === 0) {
        delete pkg[key]
      }
    }
    packages[depPath] = pkg as PackageSnapshot
  }
  newLockfile.packages = packages

  return newLockfile
}

type ValueMerger<T> = (ourValue: T, theirValue: T) => T

function mergeDict<T> (
  ourDict: Record<string, T>,
  theirDict: Record<string, T>,
  valueMerger: ValueMerger<T>
) {
  const newDict: Record<string, T> = {}
  for (const key of Object.keys(ourDict).concat(Object.keys(theirDict))) {
    const changedValue = valueMerger(
      ourDict[key],
      theirDict[key]
    )
    if (changedValue) {
      newDict[key] = changedValue
    }
  }
  return newDict
}

function takeChangedValue<T> (ourValue: T, theirValue: T): T {
  if (ourValue === theirValue || theirValue == null) return ourValue
  return theirValue
}

function mergeVersions (ourValue: string, theirValue: string) {
  if (ourValue === theirValue || !theirValue) return ourValue
  if (!ourValue) return theirValue
  const [ourVersion] = ourValue.split('_')
  const [theirVersion] = theirValue.split('_')
  if (semver.gt(ourVersion, theirVersion)) {
    return ourValue
  }
  return theirValue
}

function mergeImporterVersions (ourValue: VersionAndSpecifier, theirValue: VersionAndSpecifier) {
  if (ourValue?.specifier === theirValue?.specifier && ourValue?.version === theirValue?.version || !theirValue) return ourValue
  if (!ourValue) return theirValue
  const [ourVersion] = ourValue.version.split('_')
  const [theirVersion] = theirValue.version.split('_')
  if (semver.gt(ourVersion, theirVersion)) {
    return ourValue
  }
  return theirValue
}
