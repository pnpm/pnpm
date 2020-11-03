import { Lockfile } from '@pnpm/lockfile-types'
import R = require('ramda')
import semver = require('semver')

export default function mergeLockfile (
  opts: {
    base: Lockfile
    ours: Lockfile
    theirs: Lockfile
  }
) {
  const newLockfile: Lockfile = {
    importers: {},
    lockfileVersion: Math.max(opts.base.lockfileVersion, opts.ours.lockfileVersion),
  }

  for (const importerId of Array.from(new Set([...Object.keys(opts.ours.importers), ...Object.keys(opts.theirs.importers)]))) {
    newLockfile.importers[importerId] = {
      specifiers: {},
    }
    for (const key of ['dependencies', 'devDependencies', 'optionalDependencies']) {
      newLockfile.importers[importerId][key] = mergeDict(
        opts.ours.importers[importerId]?.[key] ?? {},
        opts.base.importers[importerId]?.[key] ?? {},
        opts.theirs.importers[importerId]?.[key] ?? {},
        key,
        mergeVersions
      )
      if (!Object.keys(newLockfile.importers[importerId][key]).length) {
        delete newLockfile.importers[importerId][key]
      }
    }
    newLockfile.importers[importerId].specifiers = mergeDict(
      opts.ours.importers[importerId]?.specifiers ?? {},
      opts.base.importers[importerId]?.specifiers ?? {},
      opts.theirs.importers[importerId]?.specifiers ?? {},
      'specifiers',
      takeChangedValue
    )
  }

  const packages = {}
  for (const depPath of Array.from(new Set([...Object.keys(opts.ours.packages ?? {}), ...Object.keys(opts.theirs.packages ?? {})]))) {
    const basePkg = opts.base.packages?.[depPath]
    const ourPkg = opts.ours.packages?.[depPath]
    const theirPkg = opts.theirs.packages?.[depPath]
    const pkg = {
      ...basePkg,
      ...ourPkg,
      ...theirPkg,
    }
    for (const key of ['dependencies', 'optionalDependencies']) {
      pkg[key] = mergeDict(
        ourPkg?.[key] ?? {},
        basePkg?.[key] ?? {},
        theirPkg?.[key] ?? {},
        key,
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

type ValueMerger<T> = (ourValue: T, baseValue: T, theirValue: T, fieldName: string) => T

function mergeDict<T> (
  ourDict: Record<string, T>,
  baseDict: Record<string, T>,
  theirDict: Record<string, T>,
  fieldName: string,
  valueMerger: ValueMerger<T>
) {
  const newDict = {}
  for (const key of R.keys(ourDict).concat(R.keys(theirDict))) {
    const changedValue = valueMerger(
      ourDict[key],
      baseDict[key],
      theirDict[key],
      `${fieldName}.${key}`
    )
    if (changedValue) {
      newDict[key] = changedValue
    }
  }
  return newDict
}

function takeChangedValue<T> (ourValue: T, baseValue: T, theirValue: T, fieldName: string): T {
  if (ourValue === theirValue) return ourValue
  if (baseValue === ourValue) return theirValue
  if (baseValue === theirValue) return ourValue
  // eslint-disable-next-line
  throw new Error(`Cannot resolve '${fieldName}'. Base value: ${baseValue}. Our: ${ourValue}. Their: ${theirValue}`)
}

function mergeVersions (ourValue: string, baseValue: string, theirValue: string, fieldName: string) {
  if (ourValue === theirValue) return ourValue
  if (baseValue === ourValue) return theirValue
  if (baseValue === theirValue) return ourValue
  const [ourVersion] = ourValue.split('_')
  const [theirVersion] = theirValue.split('_')
  if (semver.gt(ourVersion, theirVersion)) {
    return ourValue
  }
  return theirValue
}
