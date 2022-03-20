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
import { install, InstallOptions } from '@pnpm/core'
import { Config } from '@pnpm/config'
import findWorkspacePackages from '@pnpm/find-workspace-packages'
import { Project } from '@pnpm/types'
import logger from '@pnpm/logger'
import { sequenceGraph } from '@pnpm/sort-packages'
import rimraf from '@zkochan/rimraf'
import loadJsonFile from 'load-json-file'
import renderHelp from 'render-help'
import { parse as parseYarnLock } from '@yarnpkg/lockfile'
import * as yarnCore from '@yarnpkg/core'
import { parseSyml } from '@yarnpkg/parsers'
import exists from 'path-exists'
import getOptionsFromRootManifest from '../getOptionsFromRootManifest'
import recursive from '../recursive'
import { yarnLockFileKeyNormalizer } from './yarnUtil'

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
  optionalDependencies?: {
    [depName: string]: string
  }
}
interface YarnPackgeLock {
  [name: string]: YarnLockPackage
}

enum YarnLockType {
  yarn = 'yarn',
  yarn2 = 'yarn2',
}

// copy from yarn v1
interface YarnLock2Struct {
  type: YarnLockType.yarn2
  object: YarnPackgeLock
}

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

export type ImportCommandOptions = Pick<Config,
| 'allProjects'
| 'selectedProjectsGraph'
| 'workspaceDir'
> & CreateStoreControllerOptions & Omit<InstallOptions, 'storeController' | 'lockfileOnly' | 'preferredVersions'>

export async function handler (
  opts: ImportCommandOptions,
  params: string[]
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

  // For a workspace with shared lockfile
  if (opts.workspaceDir) {
    const allProjects = opts.allProjects ?? await findWorkspacePackages(opts.workspaceDir, opts)
    const selectedProjectsGraph = opts.selectedProjectsGraph ?? selectProjectByDir(allProjects, opts.dir)
    if (selectedProjectsGraph != null) {
      const sequencedGraph = sequenceGraph(selectedProjectsGraph)
      // Check and warn if there are cyclic dependencies
      if (!sequencedGraph.safe) {
        const cyclicDependenciesInfo = sequencedGraph.cycles.length > 0
          ? `: ${sequencedGraph.cycles.map(deps => deps.join(', ')).join('; ')}` // eslint-disable-line
          : ''
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
  const installOpts = {
    ...opts,
    ...getOptionsFromRootManifest(manifest),
    lockfileOnly: true,
    preferredVersions,
    storeController: store.ctrl,
    storeDir: store.dir,
  }
  await install(manifest, installOpts)
}

async function readYarnLockFile (dir: string) {
  try {
    const yarnLockFile = await gfs.readFile(path.join(dir, 'yarn.lock'), 'utf8')
    let lockJsonFile
    const yarnLockFileType = getYarnLockfileType(yarnLockFile)
    if (yarnLockFileType === YarnLockType.yarn) {
      lockJsonFile = await parseYarnLock(yarnLockFile)
      if (lockJsonFile.type === 'success') {
        return lockJsonFile.object
      } else {
        throw new PnpmError('YARN_LOCKFILE_PARSE_FAILED', `Yarn.lock file was ${lockJsonFile.type}`)
      }
    } else if (yarnLockFileType === YarnLockType.yarn2) {
      lockJsonFile = parseYarn2Lock(yarnLockFile)
      if (lockJsonFile.type === YarnLockType.yarn2) {
        return lockJsonFile.object
      }
    }
  } catch (err: any) { // eslint-disable-line
    if (err['code'] !== 'ENOENT') throw err // eslint-disable-line @typescript-eslint/dot-notation
  }
  throw new PnpmError('YARN_LOCKFILE_NOT_FOUND', 'No yarn.lock found')
}

function parseYarn2Lock (lockFileContents: string): YarnLock2Struct {
  // eslint-disable-next-line
  const parseYarnLock: any = parseSyml(lockFileContents)

  delete parseYarnLock.__metadata
  const dependencies: YarnPackgeLock = {}

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

async function readNpmLockfile (dir: string) {
  try {
    return await loadJsonFile<LockedPackage>(path.join(dir, 'package-lock.json'))
  } catch (err: any) { // eslint-disable-line
    if (err['code'] !== 'ENOENT') throw err // eslint-disable-line @typescript-eslint/dot-notation
  }
  try {
    return await loadJsonFile<LockedPackage>(path.join(dir, 'npm-shrinkwrap.json'))
  } catch (err: any) { // eslint-disable-line
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

function selectProjectByDir (projects: Project[], searchedDir: string) {
  const project = projects.find(({ dir }) => path.relative(dir, searchedDir) === '')
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
