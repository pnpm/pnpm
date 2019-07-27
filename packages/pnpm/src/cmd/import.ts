import { WANTED_LOCKFILE } from '@pnpm/constants'
import PnpmError from '@pnpm/error'
import { readImporterManifestOnly } from '@pnpm/read-importer-manifest'
import loadJsonFile = require('load-json-file')
import path = require('path')
import rimraf = require('rimraf-then')
import { install } from 'supi'
import createStoreController from '../createStoreController'
import { PnpmOptions } from '../types'

export default async function installCmd (
  input: string[],
  opts: PnpmOptions,
) {
  // Removing existing pnpm lockfile
  // it should not influence the new one
  await rimraf(path.join(opts.prefix, WANTED_LOCKFILE))
  const npmPackageLock = await readNpmLockfile(opts.prefix)
  const versionsByPackageNames = {}
  getAllVersionsByPackageNames(npmPackageLock, versionsByPackageNames)
  const preferredVersions = getPreferredVersions(versionsByPackageNames)
  const store = await createStoreController(opts)
  const installOpts = {
    ...opts,
    lockfileOnly: true,
    preferredVersions,
    store: store.path,
    storeController: store.ctrl,
  }
  await install(await readImporterManifestOnly(opts.prefix), installOpts)
}

async function readNpmLockfile (prefix: string) {
  try {
    return await loadJsonFile<LockedPackage>(path.join(prefix, 'package-lock.json'))
  } catch (err) {
    if (err['code'] !== 'ENOENT') throw err // tslint:disable-line:no-string-literal
  }
  try {
    return await loadJsonFile<LockedPackage>(path.join(prefix, 'npm-shrinkwrap.json'))
  } catch (err) {
    if (err['code'] !== 'ENOENT') throw err // tslint:disable-line:no-string-literal
  }
  throw new PnpmError('NPM_LOCKFILE_NOT_FOUND', 'No package-lock.json or npm-shrinkwrap.json found')
}

function getPreferredVersions (
  versionsByPackageNames: {
    [packageName: string]: Set<string>,
  },
) {
  const preferredVersions = {}
  for (const packageName of Object.keys(versionsByPackageNames)) {
    if (versionsByPackageNames[packageName].size === 1) {
      preferredVersions[packageName] = {
        selector: Array.from(versionsByPackageNames[packageName])[0],
        type: 'version',
      }
    } else {
      preferredVersions[packageName] = {
        selector: Array.from(versionsByPackageNames[packageName]).join(' || '),
        type: 'range',
      }
    }
  }
  return preferredVersions
}

function getAllVersionsByPackageNames (
  npmPackageLock: NpmPackageLock | LockedPackage,
  versionsByPackageNames: {
    [packageName: string]: Set<string>,
  },
) {
  if (!npmPackageLock.dependencies) return
  for (const packageName of Object.keys(npmPackageLock.dependencies)) {
    if (!versionsByPackageNames[packageName]) {
      versionsByPackageNames[packageName] = new Set()
    }
    versionsByPackageNames[packageName].add(npmPackageLock.dependencies[packageName].version)
  }
  for (const packageName of Object.keys(npmPackageLock.dependencies)) {
    getAllVersionsByPackageNames(npmPackageLock.dependencies[packageName], versionsByPackageNames)
  }
}

interface NpmPackageLock {
  dependencies: LockedPackagesMap,
}

interface LockedPackage {
  version: string,
  dependencies?: LockedPackagesMap,
}

interface LockedPackagesMap {
  [name: string]: LockedPackage,
}
