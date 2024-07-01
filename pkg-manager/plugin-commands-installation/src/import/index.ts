import path from 'path'
import { docsUrl } from '@pnpm/cli-utils'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import { PnpmError } from '@pnpm/error'
import { readProjectManifestOnly } from '@pnpm/read-project-manifest'
import {
  createOrConnectStoreController,
  type CreateStoreControllerOptions,
} from '@pnpm/store-connection-manager'
import gfs from '@pnpm/graceful-fs'
import { install, type InstallOptions } from '@pnpm/core'
import { type Config, getOptionsFromRootManifest } from '@pnpm/config'
import { findWorkspacePackages } from '@pnpm/workspace.find-packages'
import { type ProjectsGraph, type Project } from '@pnpm/types'
import { logger } from '@pnpm/logger'
import { sequenceGraph } from '@pnpm/sort-packages'
import rimraf from '@zkochan/rimraf'
import loadJsonFile from 'load-json-file'
import mapValues from 'ramda/src/map'
import renderHelp from 'render-help'
import { parse as parseYarnLock, type LockFileObject } from '@yarnpkg/lockfile'
import * as yarnCore from '@yarnpkg/core'
import { parseSyml } from '@yarnpkg/parsers'
import exists from 'path-exists'
import { recursive } from '../recursive'
import { yarnLockFileKeyNormalizer } from './yarnUtil'

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

enum YarnLockType {
  yarn = 'yarn',
  yarn2 = 'yarn2'
}

// copy from yarn v1
interface YarnLock2Struct {
  type: YarnLockType.yarn2
  object: YarnPackageLock
}

export const rcOptionsTypes = cliOptionsTypes

export function cliOptionsTypes (): Record<string, unknown> {
  return {}
}

export function help (): string {
  return renderHelp({
    description: `Generates ${WANTED_LOCKFILE} from an npm package-lock.json (or npm-shrinkwrap.json, yarn.lock) file.`,
    url: docsUrl('import'),
    usages: [
      'pnpm import',
    ],
  })
}

export const commandNames = ['import']

export type ImportCommandOptions = Pick<Config,
| 'allProjects'
| 'allProjectsGraph'
| 'selectedProjectsGraph'
| 'workspaceDir'
| 'ignoreWorkspaceCycles'
| 'disallowWorkspaceCycles'
| 'sharedWorkspaceLockfile'
| 'workspacePackagePatterns'
| 'rootProjectManifest'
| 'rootProjectManifestDir'
> & CreateStoreControllerOptions & Omit<InstallOptions, 'storeController' | 'lockfileOnly' | 'preferredVersions'>

export async function handler (
  opts: ImportCommandOptions,
  params: string[]
): Promise<void> {
  // Removing existing pnpm lockfile
  // it should not influence the new one
  await rimraf(path.join(opts.dir, WANTED_LOCKFILE))
  const versionsByPackageNames = {}
  let preferredVersions = {}
  if (await exists(path.join(opts.dir, 'yarn.lock'))) {
    const yarnPackageLockFile = await readYarnLockFile(opts.dir)
    getAllVersionsFromYarnLockFile(yarnPackageLockFile, versionsByPackageNames)
  } else if (
    await exists(path.join(opts.dir, 'package-lock.json')) ||
    await exists(path.join(opts.dir, 'npm-shrinkwrap.json'))
  ) {
    const npmPackageLock = await readNpmLockfile(opts.dir)
    if (npmPackageLock.lockfileVersion < 3) {
      getAllVersionsByPackageNamesPreV3(npmPackageLock, versionsByPackageNames)
    } else {
      getAllVersionsByPackageNames(npmPackageLock, versionsByPackageNames)
    }
  } else {
    throw new PnpmError('LOCKFILE_NOT_FOUND', 'No lockfile found')
  }
  preferredVersions = getPreferredVersions(versionsByPackageNames)

  // For a workspace with shared lockfile
  if (opts.workspaceDir) {
    const allProjects = opts.allProjects ?? await findWorkspacePackages(opts.workspaceDir, {
      ...opts,
      patterns: opts.workspacePackagePatterns,
    })
    const selectedProjectsGraph = opts.selectedProjectsGraph ?? selectProjectByDir(allProjects, opts.dir)
    if (selectedProjectsGraph != null) {
      const sequencedGraph = sequenceGraph(selectedProjectsGraph)
      // Check and warn if there are cyclic dependencies
      if (!opts.ignoreWorkspaceCycles && !sequencedGraph.safe) {
        const cyclicDependenciesInfo = sequencedGraph.cycles.length > 0
          ? `: ${sequencedGraph.cycles.map(deps => deps.join(', ')).join('; ')}`
          : ''

        if (opts.disallowWorkspaceCycles) {
          throw new PnpmError('DISALLOW_WORKSPACE_CYCLES', `There are cyclic workspace dependencies${cyclicDependenciesInfo}`)
        }

        logger.warn({
          message: `There are cyclic workspace dependencies${cyclicDependenciesInfo}`,
          prefix: opts.workspaceDir,
        })
      }
      await recursive(allProjects,
        params,
        // @ts-expect-error
        {
          ...opts,
          lockfileOnly: true,
          selectedProjectsGraph,
          preferredVersions,
          workspaceDir: opts.workspaceDir,
        },
        'import'
      )
    }
    return
  }

  const store = await createOrConnectStoreController(opts)
  const manifest = await readProjectManifestOnly(opts.dir)
  const manifestOpts = opts.rootProjectManifest ? getOptionsFromRootManifest(opts.rootProjectManifestDir, opts.rootProjectManifest) : {}
  const installOpts = {
    ...opts,
    ...manifestOpts,
    lockfileOnly: true,
    preferredVersions,
    storeController: store.ctrl,
    storeDir: store.dir,
  }
  await install(manifest, installOpts)
}

async function readYarnLockFile (dir: string): Promise<LockFileObject> {
  try {
    const yarnLockFile = await gfs.readFile(path.join(dir, 'yarn.lock'), 'utf8')
    const yarnLockFileType = getYarnLockfileType(yarnLockFile)
    if (yarnLockFileType === YarnLockType.yarn) {
      const lockJsonFile = parseYarnLock(yarnLockFile)
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
  const parseYarnLock = parseSyml(lockFileContents)

  delete parseYarnLock.__metadata
  const dependencies: YarnPackageLock = {}

  const { structUtils } = yarnCore
  const { parseDescriptor, parseRange } = structUtils
  const keyNormalizer = yarnLockFileKeyNormalizer(
    parseDescriptor,
    parseRange
  )

  Object.entries(parseYarnLock).forEach(
    // eslint-disable-next-line
    ([fullDescriptor, versionData]: [string, any]) => {
      keyNormalizer(fullDescriptor).forEach((descriptor) => {
        dependencies[descriptor] = versionData
      })
    }
  )
  return {
    object: dependencies,
    type: YarnLockType.yarn2,
  }
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

function getPreferredVersions (versionsByPackageNames: VersionsByPackageNames): Record<string, Record<string, string>> {
  const preferredVersions = mapValues(
    (versions) => Object.fromEntries(Array.from(versions).map((version) => [version, 'version'])),
    versionsByPackageNames
  )
  return preferredVersions
}

type VersionsByPackageNames = Record<string, Set<string>>

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
  for (let [pkgName, pkgDetails] of Object.entries(dependencies)) {
    if (pkgName.includes('node_modules')) {
      pkgName = pkgName.substring(pkgName.lastIndexOf('node_modules/') + 13)
    }
    if (!versionsByPackageNames[pkgName]) {
      versionsByPackageNames[pkgName] = new Set<string>()
    }
    if (pkgDetails.version) {
      versionsByPackageNames[pkgName].add(pkgDetails.version)
    }

    if (pkgDetails.packages) {
      extractDependencies(versionsByPackageNames, pkgDetails.packages)
    }
    if (pkgDetails.dependencies) {
      for (const [pkgName1, version] of Object.entries(pkgDetails.dependencies)) {
        if (!versionsByPackageNames[pkgName1]) {
          versionsByPackageNames[pkgName1] = new Set<string>()
        }
        versionsByPackageNames[pkgName1].add(version)
      }
    }
  }
}

function getAllVersionsFromYarnLockFile (
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

function selectProjectByDir (projects: Project[], searchedDir: string): ProjectsGraph | undefined {
  const project = projects.find(({ rootDir }) => path.relative(rootDir, searchedDir) === '')
  if (project == null) return undefined
  return { [searchedDir]: { dependencies: [], package: project } }
}

function getYarnLockfileType (
  lockFileContents: string
): YarnLockType {
  return lockFileContents.includes('__metadata')
    ? YarnLockType.yarn2
    : YarnLockType.yarn
}
