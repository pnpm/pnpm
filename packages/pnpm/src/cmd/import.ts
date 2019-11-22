import { docsUrl } from '@pnpm/cli-utils'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import PnpmError from '@pnpm/error'
import { readImporterManifestOnly } from '@pnpm/read-importer-manifest'
import rimraf = require('@zkochan/rimraf')
import loadJsonFile = require('load-json-file')
import path = require('path')
import R = require('ramda')
import renderHelp = require('render-help')
import { install } from 'supi'
import createStoreController from '../createStoreController'
import { PnpmOptions } from '../types'

export function types () {
  return {}
}

export function help () {
  return renderHelp({
    description: `Generates ${WANTED_LOCKFILE} from an npm package-lock.json (or npm-shrinkwrap.json) file.`,
    url: docsUrl('import'),
    usages: ['pnpm import'],
  })
}

export const commandNames = ['import']

export async function handler (
  input: string[],
  opts: PnpmOptions,
) {
  // Removing existing pnpm lockfile
  // it should not influence the new one
  await rimraf(path.join(opts.dir, WANTED_LOCKFILE))
  const npmPackageLock = await readNpmLockfile(opts.dir)
  const versionsByPackageNames = {}
  getAllVersionsByPackageNames(npmPackageLock, versionsByPackageNames)
  const preferredVersions = getPreferredVersions(versionsByPackageNames)
  const store = await createStoreController(opts)
  const installOpts = {
    ...opts,
    lockfileOnly: true,
    preferredVersions,
    storeController: store.ctrl,
    storeDir: store.dir,
  }
  await install(await readImporterManifestOnly(opts.dir), installOpts)
}

async function readNpmLockfile (dir: string) {
  try {
    return await loadJsonFile<LockedPackage>(path.join(dir, 'package-lock.json'))
  } catch (err) {
    if (err['code'] !== 'ENOENT') throw err // tslint:disable-line:no-string-literal
  }
  try {
    return await loadJsonFile<LockedPackage>(path.join(dir, 'npm-shrinkwrap.json'))
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
