import { randomUUID } from 'node:crypto'
import fs from 'node:fs'
import path from 'node:path'

import { docsUrl } from '@pnpm/cli.utils'
import type { Config, ConfigContext } from '@pnpm/config.reader'
import { LOCKFILE_VERSION, WANTED_LOCKFILE } from '@pnpm/constants'
import { isError, PnpmError } from '@pnpm/error'
import { install, type InstallOptions } from '@pnpm/installing.deps-installer'
import { getLockfileImporterId, getWantedLockfileName, readEnvLockfile, writeEnvLockfile, writeWantedLockfile } from '@pnpm/lockfile.fs'
import { logger } from '@pnpm/logger'
import { EXISTING_VERSION_SELECTOR_WEIGHT, type PreferredVersions } from '@pnpm/resolving.resolver-base'
import {
  createStoreController,
  type CreateStoreControllerOptions,
} from '@pnpm/store.connection-manager'
import type { Project, ProjectsGraph } from '@pnpm/types'
import { readProjectManifest, readProjectManifestOnly } from '@pnpm/workspace.project-manifest-reader'
import { findWorkspaceProjects } from '@pnpm/workspace.projects-reader'
import { sequenceGraph } from '@pnpm/workspace.projects-sorter'
import { map as mapValues } from 'ramda'
import { renderHelp } from 'render-help'

import { recursive } from '../recursive.js'
import { getAllVersionsFromYarnLockFile, readVersionsByPackageNames, readYarnLockFile, type VersionsByPackageNames } from './lockfileVersions.js'
import { type ImportedProject, importYarnPatches } from './yarnPatches.js'

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

export const recursiveByDefault = true

export type ImportCommandOptions = Pick<Config,
| 'workspaceDir'
| 'ignoreWorkspaceCycles'
| 'disallowWorkspaceCycles'
| 'sharedWorkspaceLockfile'
| 'workspacePackagePatterns'
> & Pick<ConfigContext,
| 'allProjects'
| 'allProjectsGraph'
| 'selectedProjectsGraph'
| 'rootProjectManifest'
| 'rootProjectManifestDir'
> & CreateStoreControllerOptions & Omit<InstallOptions, 'storeController' | 'lockfileOnly' | 'preferredVersions'>

export async function handler (
  opts: ImportCommandOptions,
  params: string[]
): Promise<void> {
  const versionsByPackageNames = await readVersionsByPackageNames(opts.dir)
  const preferredVersions = getPreferredVersions(versionsByPackageNames)
  const projects = await getImportedProjects(opts)
  const preferredVersionsByImporterId = await nestedYarnLockPreferredVersions(opts, projects)
  const patchedDependencies = await importYarnPatches({
    projects,
    yarnRootDir: opts.dir,
    workspaceDir: opts.workspaceDir ?? opts.dir,
    patchedDependencies: opts.patchedDependencies,
  }) ?? opts.patchedDependencies
  await replaceWantedLockfile(opts, async () => installImportedLockfile(
    { ...opts, patchedDependencies },
    params,
    { preferredVersions, preferredVersionsByImporterId }
  ))
}

async function replaceWantedLockfile (
  opts: Pick<ImportCommandOptions, 'dir' | 'lockfileDir' | 'useGitBranchLockfile' | 'mergeGitBranchLockfiles'>,
  writeImportedLockfile: () => Promise<void>
): Promise<void> {
  const lockfileDir = opts.lockfileDir ?? opts.dir
  // Resolved the way the installer resolves it, so the backed up file and the
  // file the import writes back are the same one.
  const lockfileName = await getWantedLockfileName({
    useGitBranchLockfile: opts.useGitBranchLockfile,
    mergeGitBranchLockfiles: opts.mergeGitBranchLockfiles,
  })
  const lockfilePath = path.join(lockfileDir, lockfileName)
  // The env document leads pnpm-lock.yaml, so a branch import has none to carry over.
  const envLockfile = lockfileName === WANTED_LOCKFILE ? await readEnvLockfile(lockfileDir) : undefined
  // A backup of its own keeps overlapping imports from restoring each other's copy.
  const backupPath = `${lockfilePath}.${randomUUID()}.import.bak`
  // The existing pnpm lockfile must not influence the imported versions.
  const lockfileExisted = await moveIfExists(lockfilePath, backupPath)
  try {
    if (lockfileName !== WANTED_LOCKFILE) {
      // An absent branch lockfile would fall back to the shared lockfile.
      await fs.promises.mkdir(lockfileDir, { recursive: true })
      await writeWantedLockfile(lockfileDir, { lockfileVersion: LOCKFILE_VERSION, importers: {} }, { lockfileName })
    }
    if (envLockfile) {
      await writeEnvLockfile(lockfileDir, envLockfile)
    }
    await writeImportedLockfile()
  } catch (err: unknown) {
    await fs.promises.rm(lockfilePath, { force: true })
    if (lockfileExisted) {
      await fs.promises.rename(backupPath, lockfilePath)
    }
    throw err
  }
  if (lockfileExisted) {
    await fs.promises.unlink(backupPath)
  }
}

async function moveIfExists (sourcePath: string, targetPath: string): Promise<boolean> {
  try {
    await fs.promises.rename(sourcePath, targetPath)
    return true
  } catch (err: unknown) {
    if (!isError(err) || !('code' in err) || err.code !== 'ENOENT') throw err
    return false
  }
}

async function getImportedProjects (opts: ImportCommandOptions): Promise<ImportedProject[]> {
  if (opts.workspaceDir) {
    return opts.allProjects ?? findWorkspaceProjects(opts.workspaceDir, {
      ...opts,
      patterns: opts.workspacePackagePatterns,
    })
  }
  return [{ rootDir: opts.dir, ...await readProjectManifest(opts.dir) }]
}

interface ImportedPreferredVersions {
  preferredVersions: PreferredVersions
  preferredVersionsByImporterId?: Record<string, PreferredVersions>
}

async function installImportedLockfile (
  opts: ImportCommandOptions,
  params: string[],
  imported: ImportedPreferredVersions
): Promise<void> {
  if (opts.workspaceDir) {
    await installImportedWorkspaceLockfile({ ...opts, workspaceDir: opts.workspaceDir }, params, imported)
    return
  }

  const store = await createStoreController(opts)
  const manifest = await readProjectManifestOnly(opts.dir)
  const installOpts = {
    ...opts,
    lockfileOnly: true,
    preferredVersions: imported.preferredVersions,
    storeController: store.ctrl,
    storeDir: store.dir,
    resolutionVerifiers: store.resolutionVerifiers,
  }
  await install(manifest, installOpts)
}

async function installImportedWorkspaceLockfile (
  opts: ImportCommandOptions & { workspaceDir: string },
  params: string[],
  imported: ImportedPreferredVersions
): Promise<void> {
  const allProjects = opts.allProjects ?? await findWorkspaceProjects(opts.workspaceDir, {
    ...opts,
    patterns: opts.workspacePackagePatterns,
  })
  const selectedProjectsGraph = opts.selectedProjectsGraph ?? selectProjectByDir(allProjects, opts.dir)
  if (selectedProjectsGraph == null) return
  checkWorkspaceCycles(opts, sequenceGraph(selectedProjectsGraph).cycles)
  await recursive(allProjects,
    params,
    // @ts-expect-error -- the import options do not declare bail and linkWorkspacePackages, which RecursiveOptions requires
    {
      ...opts,
      lockfileOnly: true,
      selectedProjectsGraph,
      preferredVersions: imported.preferredVersions,
      preferredVersionsByImporterId: imported.preferredVersionsByImporterId,
      workspaceDir: opts.workspaceDir,
    },
    'import'
  )
}

function checkWorkspaceCycles (
  opts: Pick<ImportCommandOptions, 'ignoreWorkspaceCycles' | 'disallowWorkspaceCycles'> & { workspaceDir: string },
  cycles: string[][]
): void {
  if (opts.ignoreWorkspaceCycles || !cycles.some((cycle) => cycle.length > 1)) return
  const cyclicDependenciesInfo = cycles.length > 0
    ? `: ${cycles.map(deps => deps.join(', ')).join('; ')}`
    : ''

  if (opts.disallowWorkspaceCycles) {
    throw new PnpmError('DISALLOW_WORKSPACE_CYCLES', `There are cyclic workspace dependencies${cyclicDependenciesInfo}`)
  }

  logger.warn({
    message: `There are cyclic workspace dependencies${cyclicDependenciesInfo}`,
    prefix: opts.workspaceDir,
  })
}

// The imported lockfile's pins must outrank the direct-dependency ranges that
// every workspace project contributes, as the pins of a pnpm lockfile do.
const IMPORTED_VERSION_SELECTOR = {
  selectorType: 'version',
  weight: EXISTING_VERSION_SELECTOR_WEIGHT,
} as const

async function nestedYarnLockPreferredVersions (
  opts: ImportCommandOptions,
  projects: ImportedProject[]
): Promise<Record<string, PreferredVersions> | undefined> {
  if (opts.workspaceDir == null) return undefined
  const lockfileDir = opts.lockfileDir ?? opts.workspaceDir
  const byImporterId: Record<string, PreferredVersions> = Object.create(null)
  await Promise.all(projects.map(async (project) => {
    if (path.relative(project.rootDir, opts.dir) === '') return
    if (!fs.existsSync(path.join(project.rootDir, 'yarn.lock'))) return
    const versionsByPackageNames: VersionsByPackageNames = Object.create(null)
    getAllVersionsFromYarnLockFile(await readYarnLockFile(project.rootDir), versionsByPackageNames)
    byImporterId[getLockfileImporterId(lockfileDir, project.rootDir)] = getPreferredVersions(versionsByPackageNames)
  }))
  return Object.keys(byImporterId).length === 0 ? undefined : byImporterId
}

function getPreferredVersions (versionsByPackageNames: VersionsByPackageNames): PreferredVersions {
  const preferredVersions = mapValues(
    (versions) => Object.fromEntries(Array.from(versions).map((version) => [version, IMPORTED_VERSION_SELECTOR])),
    versionsByPackageNames
  )
  return preferredVersions
}

function selectProjectByDir (projects: Project[], searchedDir: string): ProjectsGraph | undefined {
  const project = projects.find(({ rootDir }) => path.relative(rootDir, searchedDir) === '')
  if (project == null) return undefined
  return { [project.rootDir]: { dependencies: [], package: project } }
}
