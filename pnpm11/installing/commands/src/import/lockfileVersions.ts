import fs from 'node:fs'
import path from 'node:path'

import { PnpmError } from '@pnpm/error'
import gfs from '@pnpm/fs.graceful-fs'
import * as structUtils from '@yarnpkg/core/structUtils'
import { type LockFileObject, parse as parseYarnLockfile } from '@yarnpkg/lockfile'
import yaml from 'js-yaml'
import { loadJsonFile } from 'load-json-file'

import { yarnLockFileKeyNormalizer } from './yarnUtil.js'

interface NpmPackageLock {
  dependencies: LockedPackagesMap
  packages: LockedPackagesMap
  name?: string
}

interface LockedPackage {
  version: string
  lockfileVersion: number
  name?: string
  dependencies?: LockedPackagesMap | SimpleDependenciesMap
  packages?: LockedPackagesMap
}

interface SimpleDependenciesMap {
  [name: string]: string
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
  optionalDependencies?: {
    [depName: string]: string
  }
}
interface YarnPackageLock {
  [name: string]: YarnLockPackage
}

type YarnLockYaml = YarnPackageLock & { __metadata?: unknown }

const YarnLockType = {
  yarn: 'yarn',
  yarn2: 'yarn2',
} as const

type YarnLockType = (typeof YarnLockType)[keyof typeof YarnLockType]

// copy from yarn v1
interface YarnLock2Struct {
  type: typeof YarnLockType.yarn2
  object: YarnPackageLock
}

export async function readVersionsByPackageNames (dir: string): Promise<VersionsByPackageNames> {
  const versionsByPackageNames: VersionsByPackageNames = Object.create(null)
  if (fs.existsSync(path.join(dir, 'yarn.lock'))) {
    const yarnPackageLockFile = await readYarnLockFile(dir)
    getAllVersionsFromYarnLockFile(yarnPackageLockFile, versionsByPackageNames)
    return versionsByPackageNames
  }
  if (
    !fs.existsSync(path.join(dir, 'package-lock.json')) &&
    !fs.existsSync(path.join(dir, 'npm-shrinkwrap.json'))
  ) {
    throw new PnpmError('LOCKFILE_NOT_FOUND', 'No lockfile found')
  }
  const npmPackageLock = await readNpmLockfile(dir)
  if (npmPackageLock.lockfileVersion < 3) {
    getAllVersionsByPackageNamesPreV3(npmPackageLock, versionsByPackageNames)
  } else {
    getAllVersionsByPackageNames(npmPackageLock, versionsByPackageNames)
  }
  return versionsByPackageNames
}

export async function readYarnLockFile (dir: string): Promise<LockFileObject> {
  try {
    const yarnLockFile = await gfs.readFile(path.join(dir, 'yarn.lock'), 'utf8')
    const yarnLockFileType = getYarnLockfileType(yarnLockFile)
    if (yarnLockFileType === YarnLockType.yarn) {
      const lockJsonFile = parseYarnLockfile(yarnLockFile)
      if (lockJsonFile.type === 'success') {
        return lockJsonFile.object
      } else {
        throw new PnpmError('YARN_LOCKFILE_PARSE_FAILED', `Yarn.lock file was ${lockJsonFile.type}`)
      }
    } else if (yarnLockFileType === YarnLockType.yarn2) {
      const lockJsonFile = parseYarn2Lock(yarnLockFile)
      if (lockJsonFile.type === YarnLockType.yarn2) {
        return lockJsonFile.object
      }
    }
  } catch (err: any) { // eslint-disable-line
    if (err['code'] !== 'ENOENT') throw err
  }
  throw new PnpmError('YARN_LOCKFILE_NOT_FOUND', 'No yarn.lock found')
}

function parseYarn2Lock (lockFileContents: string): YarnLock2Struct {
  const parseYarnLock = parseYarn2Yaml(lockFileContents)

  delete parseYarnLock.__metadata
  const dependencies: YarnPackageLock = {}

  const { parseDescriptor, parseRange } = structUtils
  const keyNormalizer = yarnLockFileKeyNormalizer(
    parseDescriptor,
    parseRange
  )

  for (const fullDescriptor in parseYarnLock) {
    const versionData = parseYarnLock[fullDescriptor]
    for (const descriptor of keyNormalizer(fullDescriptor)) {
      dependencies[descriptor] = versionData
    }
  }
  return {
    object: dependencies,
    type: YarnLockType.yarn2,
  }
}

function parseYarn2Yaml (lockFileContents: string): YarnLockYaml {
  const parseYarnLock = yaml.load(lockFileContents, {
    schema: yaml.FAILSAFE_SCHEMA,
    json: true,
  })
  if (parseYarnLock == null) return {}
  if (typeof parseYarnLock !== 'object' || Array.isArray(parseYarnLock)) {
    throw new PnpmError('YARN_LOCKFILE_PARSE_FAILED', `Expected an indexed object, got ${Array.isArray(parseYarnLock) ? 'an array' : `a ${typeof parseYarnLock}`} instead. Does your file follow YAML's rules?`)
  }
  return parseYarnLock as YarnLockYaml
}

async function readNpmLockfile (dir: string): Promise<LockedPackage> {
  try {
    return await loadJsonFile<LockedPackage>(path.join(dir, 'package-lock.json'))
  } catch (err: any) { // eslint-disable-line
    if (err['code'] !== 'ENOENT') throw err
  }
  try {
    return await loadJsonFile<LockedPackage>(path.join(dir, 'npm-shrinkwrap.json'))
  } catch (err: any) { // eslint-disable-line
    if (err['code'] !== 'ENOENT') throw err
  }
  throw new PnpmError('NPM_LOCKFILE_NOT_FOUND', 'No package-lock.json or npm-shrinkwrap.json found')
}

export type VersionsByPackageNames = Record<string, Set<string>>

function getAllVersionsByPackageNamesPreV3 (
  npmPackageLock: NpmPackageLock | LockedPackage,
  versionsByPackageNames: VersionsByPackageNames
): void {
  if (npmPackageLock.dependencies == null) return
  for (const [packageName, { version }] of Object.entries(npmPackageLock.dependencies)) {
    if (!versionsByPackageNames[packageName]) {
      versionsByPackageNames[packageName] = new Set()
    }
    versionsByPackageNames[packageName].add(version)
  }
  for (const dep of Object.values(npmPackageLock.dependencies)) {
    getAllVersionsByPackageNamesPreV3(dep, versionsByPackageNames)
  }
}

function getAllVersionsByPackageNames (
  pkg: NpmPackageLock | LockedPackage,
  versionsByPackageNames: VersionsByPackageNames
): void {
  if (pkg.dependencies) {
    extractDependencies(versionsByPackageNames, pkg.dependencies as LockedPackagesMap)
  }
  if ('packages' in pkg && pkg.packages) {
    extractDependencies(versionsByPackageNames, pkg.packages)
  }
}

function extractDependencies (
  versionsByPackageNames: VersionsByPackageNames,
  dependencies: LockedPackagesMap
): void {
  for (const [pkgPath, pkgDetails] of Object.entries(dependencies)) {
    const versions = getVersionsOfPackage(versionsByPackageNames, getPackageNameFromLockfileKey(pkgPath))
    if (pkgDetails.version) {
      versions.add(pkgDetails.version)
    }

    if (pkgDetails.packages) {
      extractDependencies(versionsByPackageNames, pkgDetails.packages)
    }
    if (pkgDetails.dependencies) {
      extractDependencyRanges(versionsByPackageNames, pkgDetails.dependencies as SimpleDependenciesMap)
    }
  }
}

function getPackageNameFromLockfileKey (lockfileKey: string): string {
  if (!lockfileKey.includes('node_modules')) return lockfileKey
  return lockfileKey.substring(lockfileKey.lastIndexOf('node_modules/') + 13)
}

function extractDependencyRanges (
  versionsByPackageNames: VersionsByPackageNames,
  dependencies: SimpleDependenciesMap
): void {
  for (const [pkgName, version] of Object.entries(dependencies)) {
    getVersionsOfPackage(versionsByPackageNames, pkgName).add(version)
  }
}

function getVersionsOfPackage (versionsByPackageNames: VersionsByPackageNames, pkgName: string): Set<string> {
  if (!versionsByPackageNames[pkgName]) {
    versionsByPackageNames[pkgName] = new Set<string>()
  }
  return versionsByPackageNames[pkgName]
}

export function getAllVersionsFromYarnLockFile (
  yarnPackageLock: LockFileObject,
  versionsByPackageNames: {
    [packageName: string]: Set<string>
  }
): void {
  for (const [packageName, { version }] of Object.entries(yarnPackageLock)) {
    const pkgName = packageName.substring(0, packageName.lastIndexOf('@'))
    if (!versionsByPackageNames[pkgName]) {
      versionsByPackageNames[pkgName] = new Set()
    }
    versionsByPackageNames[pkgName].add(version)
  }
}

function getYarnLockfileType (
  lockFileContents: string
): YarnLockType {
  return lockFileContents.includes('__metadata')
    ? YarnLockType.yarn2
    : YarnLockType.yarn
}
