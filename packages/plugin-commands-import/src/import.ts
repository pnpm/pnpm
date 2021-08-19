import path from 'path'
import { docsUrl } from '@pnpm/cli-utils'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import PnpmError from '@pnpm/error'
import { readProjectManifestOnly } from '@pnpm/read-project-manifest'
import {
  createOrConnectStoreController,
  CreateStoreControllerOptions,
} from '@pnpm/store-connection-manager'
import gfs from '@pnpm/graceful-fs'
import { install, InstallOptions } from 'supi'
import rimraf from '@zkochan/rimraf'
import loadJsonFile from 'load-json-file'
import renderHelp from 'render-help'
import { parse as parseYarnLock } from '@yarnpkg/lockfile'
import exists from 'path-exists'

export const rcOptionsTypes = cliOptionsTypes

export function cliOptionsTypes () {
  return {}
}

export function help () {
  return renderHelp({
    description: `Generates ${WANTED_LOCKFILE} from an npm package-lock.json (or npm-shrinkwrap.json, yarn.lock) file.`,
    url: docsUrl('import'),
    usages: [
      'pnpm import',
    ],
  })
}

export const commandNames = ['import']

export async function handler (
  opts: CreateStoreControllerOptions & Omit<InstallOptions, 'storeController' | 'lockfileOnly' | 'preferredVersions'>
) {
  // Removing existing pnpm lockfile
  // it should not influence the new one
  await rimraf(path.join(opts.dir, WANTED_LOCKFILE))
  const versionsByPackageNames = {}
  let preferredVersions = {}
  if (await exists(path.join(opts.dir, 'yarn.lock'))) {
    const yarnPackgeLockFile = await readYarnLockFile(opts.dir)
    getAllVersionsFromYarnLockFile(yarnPackgeLockFile, versionsByPackageNames)
  } else if (
    await exists(path.join(opts.dir, 'package-lock.json')) ||
    await exists(path.join(opts.dir, 'npm-shrinkwrap.json'))
  ) {
    const npmPackageLock = await readNpmLockfile(opts.dir)
    getAllVersionsByPackageNames(npmPackageLock, versionsByPackageNames)
  } else {
    throw new PnpmError('LOCKFILE_NOT_FOUND', 'No lockfile found')
  }
  preferredVersions = getPreferredVersions(versionsByPackageNames)
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

async function readYarnLockFile (dir: string) {
  try {
    const yarnLockFile = await gfs.readFile(path.join(dir, 'yarn.lock'), 'utf8')
    const lockJsonFile = await parseYarnLock(yarnLockFile)
    if (lockJsonFile.type === 'success') {
      return lockJsonFile.object
    } else {
      throw new PnpmError('GET_YARN_LOCKFILE_ERR', `Failed With ${lockJsonFile.type}`)
    }
  } catch (err) {
    if (err['code'] !== 'ENOENT') throw err // eslint-disable-line @typescript-eslint/dot-notation
  }
  throw new PnpmError('YARN_LOCKFILE_NOT_FOUND', 'No yarn.lock found')
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
  throw new PnpmError('NPM_LOCKFILE_NOT_FOUND', 'No package-lock.json or npm-shrinkwrap.json found')
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
  if (npmPackageLock.dependencies == null) return
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

function getAllVersionsFromYarnLockFile (
  yarnPackageLock: YarnPackgeLock,
  versionsByPackageNames: {
    [packageName: string]: Set<string>
  }
) {
  for (const packageName of Object.keys(yarnPackageLock)) {
    const pkgName = packageName.substring(0, packageName.lastIndexOf('@'))
    if (!versionsByPackageNames[pkgName]) {
      versionsByPackageNames[pkgName] = new Set()
    }
    versionsByPackageNames[pkgName].add(yarnPackageLock[packageName].version)
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

interface YarnLockPackage {
  version: string
  resolved: string
  integrity: string
  dependencies?: {
    [name: string]: string
  }
}
interface YarnPackgeLock {
  [name: string]: YarnLockPackage
}
