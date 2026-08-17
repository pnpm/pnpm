import { promises as fs } from 'node:fs'
import path from 'node:path'
import util from 'node:util'

import { confirm } from '@inquirer/prompts'
import { PnpmError } from '@pnpm/error'
import type {
  IncludedDependencies,
  Modules,
} from '@pnpm/installing.modules-yaml'
import { logger } from '@pnpm/logger'
import {
  DEPENDENCIES_FIELDS,
  type ProjectRootDir,
  type RegistriesByScope,
} from '@pnpm/types'
import { rimraf } from '@zkochan/rimraf'
import { isSubdir } from 'is-subdir'
import { pathAbsolute } from 'path-absolute'
import { equals } from 'ramda'

import { checkCompatibility } from './checkCompatibility/index.js'

interface ImporterToPurge {
  modulesDir: string
  rootDir: ProjectRootDir
  rootDirRealPath?: string
}

interface SafeImporterToPurge extends ImporterToPurge {
  purgeDir: string
}

export async function validateModules (
  modules: Modules,
  projects: Array<{
    modulesDir: string
    id: string
    rootDir: ProjectRootDir
  }>,
  opts: {
    currentHoistPattern?: string[]
    currentPublicHoistPattern?: string[]
    forceNewModules: boolean
    include?: IncludedDependencies
    lockfileDir: string
    modulesDir: string
    registriesByScope: RegistriesByScope
    storeDir: string
    virtualStoreDir: string
    virtualStoreDirMaxLength: number
    confirmModulesPurge?: boolean

    hoistPattern?: string[] | undefined

    publicHoistPattern?: string[] | undefined
    global?: boolean
  }
): Promise<{ purged: boolean }> {
  const rootProject = projects.find(({ id }) => id === '.')
  if (opts.virtualStoreDirMaxLength !== modules.virtualStoreDirMaxLength) {
    if (opts.forceNewModules && (rootProject != null)) {
      return { purged: await purgeModulesDirsOfImporter(opts, rootProject) }
    }
    throw new PnpmError(
      'VIRTUAL_STORE_DIR_MAX_LENGTH_DIFF',
      'This modules directory was created using a different virtual-store-dir-max-length value.' +
      ' Run "pnpm install" to recreate the modules directory.'
    )
  }
  // virtualStoreOnly installs (e.g. `pnpm fetch`) force empty hoist patterns
  // into .modules.yaml; the follow-up install must complete linking, not purge.
  if (
    !modules.virtualStoreOnly &&
    !equals(modules.publicHoistPattern ?? [], opts.publicHoistPattern ?? [])
  ) {
    if (opts.forceNewModules && (rootProject != null)) {
      return { purged: await purgeModulesDirsOfImporter(opts, rootProject) }
    }
    throw new PnpmError(
      'PUBLIC_HOIST_PATTERN_DIFF',
      'This modules directory was created using a different public-hoist-pattern value.' +
      ' Run "pnpm install" to recreate the modules directory.'
    )
  }

  const importersToPurge: ImporterToPurge[] = []

  if (!modules.virtualStoreOnly && rootProject != null) {
    try {
      if (!equals(opts.currentHoistPattern ?? [], opts.hoistPattern ?? [])) {
        throw new PnpmError(
          'HOIST_PATTERN_DIFF',
          'This modules directory was created using a different hoist-pattern value.' +
          ' Run "pnpm install" to recreate the modules directory.'
        )
      }
    } catch (err: any) { // eslint-disable-line
      if (!opts.forceNewModules) throw err
      importersToPurge.push(rootProject)
    }
  }
  for (const project of projects) {
    try {
      checkCompatibility(modules, {
        modulesDir: project.modulesDir,
        storeDir: opts.storeDir,
        virtualStoreDir: opts.virtualStoreDir,
      })
      if (opts.lockfileDir !== project.rootDir && (opts.include != null) && modules.included) {
        for (const depsField of DEPENDENCIES_FIELDS) {
          if (opts.include[depsField] !== modules.included[depsField]) {
            throw new PnpmError('INCLUDED_DEPS_CONFLICT',
              `modules directory (at "${opts.lockfileDir}") was installed with ${stringifyIncludedDeps(modules.included)}. ` +
              `Current install wants ${stringifyIncludedDeps(opts.include)}.`
            )
          }
        }
      }
    } catch (err: any) { // eslint-disable-line
      if (!opts.forceNewModules) throw err
      importersToPurge.push(project)
    }
  }
  if (importersToPurge.length > 0 && (rootProject == null)) {
    importersToPurge.push({
      modulesDir: pathAbsolute(opts.modulesDir, opts.lockfileDir),
      rootDir: opts.lockfileDir as ProjectRootDir,
    })
  }

  const purged = importersToPurge.length > 0 &&
    await purgeModulesDirsOfImporters(opts, importersToPurge)

  return { purged }
}

async function purgeModulesDirsOfImporter (
  opts: {
    confirmModulesPurge?: boolean
    virtualStoreDir: string
  },
  importer: ImporterToPurge
): Promise<boolean> {
  return purgeModulesDirsOfImporters(opts, [importer])
}

async function purgeModulesDirsOfImporters (
  opts: {
    confirmModulesPurge?: boolean
    virtualStoreDir: string
  },
  importers: ImporterToPurge[]
): Promise<boolean> {
  const safeImporters = (await Promise.all(importers.map(resolveSafePurgeTarget)))
    .filter((importer): importer is SafeImporterToPurge => importer != null)
  if (safeImporters.length === 0) return true

  if (opts.confirmModulesPurge ?? true) {
    if (!process.stdin.isTTY) {
      throw new PnpmError('ABORTED_REMOVE_MODULES_DIR_NO_TTY', 'Aborted removal of modules directory due to no TTY', {
        hint: 'If you are running pnpm in CI, set the CI environment variable to "true", or set "confirmModulesPurge" to "false".',
      })
    }
    let confirmed: boolean
    try {
      confirmed = await confirm({
        message: safeImporters.length === 1
          ? `The modules directory at "${safeImporters[0].modulesDir}" will be removed and reinstalled from scratch. Proceed?`
          : 'The modules directories will be removed and reinstalled from scratch. Proceed?',
        default: true,
      })
    } catch (err: unknown) {
      if (err instanceof Error && err.name === 'ExitPromptError') {
        throw new PnpmError('ABORTED_REMOVE_MODULES_DIR', 'Aborted removal of modules directory')
      }
      throw err
    }
    if (!confirmed) {
      throw new PnpmError('ABORTED_REMOVE_MODULES_DIR', 'Aborted removal of modules directory')
    }
  }
  await Promise.all(safeImporters.map(async (importer) => {
    logger.info({
      message: `Recreating ${importer.modulesDir}`,
      prefix: importer.rootDir,
    })
    try {
      // We don't remove the actual modules directory, just the contents of it.
      // 1. we will need the directory anyway.
      // 2. in some setups, pnpm won't even have permission to remove the modules directory.
      await removeContentsOfDir(importer.purgeDir, opts.virtualStoreDir)
    } catch (err: any) { // eslint-disable-line
      if (err.code !== 'ENOENT') throw err
    }
  }))
  return true
}

async function resolveSafePurgeTarget (
  importer: ImporterToPurge
): Promise<SafeImporterToPurge | null> {
  const projectRootDir = await fs.realpath(importer.rootDirRealPath ?? importer.rootDir)
  let purgeDir: string
  try {
    purgeDir = await fs.realpath(importer.modulesDir)
  } catch (err: unknown) {
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') return null
    throw err
  }
  if (dirsAreEqual(projectRootDir, purgeDir) || !isSubdir(projectRootDir, purgeDir)) {
    throw new PnpmError(
      'UNSAFE_MODULES_DIR',
      `Refusing to remove the modules directory at "${importer.modulesDir}" because its resolved target is not a strict subdirectory of the project root at "${importer.rootDir}".`
    )
  }
  return { ...importer, purgeDir }
}

async function removeContentsOfDir (dir: string, virtualStoreDir: string): Promise<void> {
  const items = await fs.readdir(dir)
  await Promise.all(items.map(async (item) => {
    // The non-pnpm related hidden files are kept
    if (
      item[0] === '.' &&
      item !== '.bin' &&
      item !== '.modules.yaml' &&
      !dirsAreEqual(path.join(dir, item), virtualStoreDir)
    ) {
      return
    }
    await rimraf(path.join(dir, item))
  }))
}

function dirsAreEqual (dir1: string, dir2: string): boolean {
  return path.relative(dir1, dir2) === ''
}

function stringifyIncludedDeps (included: IncludedDependencies): string {
  return DEPENDENCIES_FIELDS.filter((depsField) => included[depsField]).join(', ')
}
