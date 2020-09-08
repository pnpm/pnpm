import path from 'path'
import fs from 'fs'
import { docsUrl } from '@pnpm/cli-utils'
import * as pnpmConst from '@pnpm/constants'
import PnpmError from '@pnpm/error'
import { readProjectManifestOnly } from '@pnpm/read-project-manifest'
import {
  createOrConnectStoreController,
  CreateStoreControllerOptions,
} from '@pnpm/store-connection-manager'
import { install, InstallOptions } from 'supi'
import { parse as parseYarnLock } from '@yarnpkg/lockfile'

import rimraf = require('@zkochan/rimraf')
import loadJsonFile = require('load-json-file')
import renderHelp = require('render-help')

export const rcOptionsTypes = cliOptionsTypes

const WANTED_LOCKFILE: string = pnpmConst.WANTED_LOCKFILE

export function cliOptionsTypes () {
  return {}
}

export function help () {
  return renderHelp({
    description: `Generates ${WANTED_LOCKFILE} from a foreign lockfile (package-lock.json or npm-shrinkwrap.json or yarn.lock).`,
    url: docsUrl('import'),
    usages: ['pnpm import'],
  })
}

export const commandNames = ['import']

export async function handler (
  opts: CreateStoreControllerOptions & Omit<InstallOptions, 'storeController' | 'lockfileOnly' | 'preferredVersions'>
) {
  // Removing existing pnpm lockfile
  // it should not influence the new one
  await rimraf(path.join(opts.dir, WANTED_LOCKFILE))
  const npmPackageLock = await readNpmLockfile(opts.dir)
  const versionsByPackageNames = {}
  getAllVersionsByPackageNames(npmPackageLock, versionsByPackageNames)
  const preferredVersions = getPreferredVersions(versionsByPackageNames)
  const store = await createOrConnectStoreController(opts)
  const installOpts = {
    ...opts,
    lockfileOnly: true,
    preferredVersions,
    storeController: store.ctrl,
    storeDir: store.dir,
  }
  await install(await readProjectManifestOnly(opts.dir), installOpts)
}

function loadYarnLockFile<T> (path: string): T {
  const o = parseYarnLock(fs.readFileSync(path, { encoding: 'utf-8' }))
  const d = Object.keys(o.object).reduce(
    (acc, key) => {
      // convert key to npm format: pkgname@version to pkgname
      acc[key.replace(/@[^@]+$/, '')] = o.object[key]
      return acc
    },
    {}
  )
  if (o.type === 'success') {
    // TODO? remove (sub-) 'dependencies' keys
    // seems only top-level versions are fixed
    return {
      version: '*', // TODO? version of root package
      dependencies: d,
    } as any
  } else {
    throw new Error('failed to parse yarn.lock')
  }
}

async function readNpmLockfile (dir: string) {
  try {
    return await loadJsonFile<LockedPackage>(path.join(dir, 'package-lock.json'))
  } catch (err) {
    if (err['code'] !== 'ENOENT') throw err // eslint-disable-line @typescript-eslint/dot-notation
  }
  try {
    return await loadJsonFile<LockedPackage>(path.join(dir, 'npm-shrinkwrap.json'))
  } catch (err) {
    if (err['code'] !== 'ENOENT') throw err // eslint-disable-line @typescript-eslint/dot-notation
  }
  try {
    return loadYarnLockFile<LockedPackage>(path.join(dir, 'yarn.lock'))
  } catch (err) {
    if (err['code'] !== 'ENOENT') throw err // eslint-disable-line @typescript-eslint/dot-notation
  }
  throw new PnpmError('NPM_LOCKFILE_NOT_FOUND', 'No foreign lockfile found: package-lock.json or npm-shrinkwrap.json or yarn.lock')
}

function getPreferredVersions (
  versionsByPackageNames: {
    [packageName: string]: Set<string>
  }
) {
  const preferredVersions = {}
  for (const packageName of Object.keys(versionsByPackageNames)) {
    preferredVersions[packageName] = Array.from(versionsByPackageNames[packageName]).reduce((acc, version) => {
      acc[version] = 'version'
      return acc
    }, {})
  }
  return preferredVersions
}

function getAllVersionsByPackageNames (
  npmPackageLock: NpmPackageLock | LockedPackage,
  versionsByPackageNames: {
    [packageName: string]: Set<string>
  }
) {
  if (npmPackageLock.dependencies === undefined) return
  for (const packageName of Object.keys(npmPackageLock.dependencies)) {
    if (versionsByPackageNames[packageName] === undefined) {
      versionsByPackageNames[packageName] = new Set()
    }
    versionsByPackageNames[packageName].add(npmPackageLock.dependencies[packageName].version)
  }
  for (const packageName of Object.keys(npmPackageLock.dependencies)) {
    getAllVersionsByPackageNames(npmPackageLock.dependencies[packageName], versionsByPackageNames)
  }
}

interface NpmPackageLock {
  dependencies: LockedPackagesMap
}

interface LockedPackage {
  version: string
  dependencies?: LockedPackagesMap
}

interface LockedPackagesMap {
  [name: string]: LockedPackage
}
