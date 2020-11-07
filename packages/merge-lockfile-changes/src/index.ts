import { Lockfile } from '@pnpm/lockfile-types'
import R = require('ramda')
import semver = require('semver')

export default function mergeLockfileChanges (ours: Lockfile, theirs: Lockfile) {
  const newLockfile: Lockfile = {
    importers: {},
    lockfileVersion: Math.max(theirs.lockfileVersion, ours.lockfileVersion),
  }

  for (const importerId of Array.from(new Set([...Object.keys(ours.importers), ...Object.keys(theirs.importers)]))) {
    newLockfile.importers[importerId] = {
      specifiers: {},
    }
    for (const key of ['dependencies', 'devDependencies', 'optionalDependencies']) {
      newLockfile.importers[importerId][key] = mergeDict(
        ours.importers[importerId]?.[key] ?? {},
        theirs.importers[importerId]?.[key] ?? {},
        mergeVersions
      )
      if (!Object.keys(newLockfile.importers[importerId][key]).length) {
        delete newLockfile.importers[importerId][key]
      }
    }
    newLockfile.importers[importerId].specifiers = mergeDict(
      ours.importers[importerId]?.specifiers ?? {},
      theirs.importers[importerId]?.specifiers ?? {},
      takeChangedValue
    )
  }

  const packages = {}
  for (const depPath of Array.from(new Set([...Object.keys(ours.packages ?? {}), ...Object.keys(theirs.packages ?? {})]))) {
    const ourPkg = ours.packages?.[depPath]
    const theirPkg = theirs.packages?.[depPath]
    const pkg = {
      ...ourPkg,
      ...theirPkg,
    }
    for (const key of ['dependencies', 'optionalDependencies']) {
      pkg[key] = mergeDict(
        ourPkg?.[key] ?? {},
        theirPkg?.[key] ?? {},
        mergeVersions
      )
      if (!Object.keys(pkg[key]).length) {
        delete pkg[key]
      }
    }
    packages[depPath] = pkg
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
  const newDict = {}
  for (const key of R.keys(ourDict).concat(R.keys(theirDict))) {
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