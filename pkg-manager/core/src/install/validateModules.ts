import { promises as fs } from 'fs'
import path from 'path'
import { PnpmError } from '@pnpm/error'
import { globalWarn, logger } from '@pnpm/logger'
import {
  type IncludedDependencies,
  type Modules,
} from '@pnpm/modules-yaml'
import {
  DEPENDENCIES_FIELDS,
  type Registries,
  type ProjectRootDir,
} from '@pnpm/types'
import rimraf from '@zkochan/rimraf'
import enquirer from 'enquirer'
import { equals } from 'ramda'
import { checkCompatibility } from './checkCompatibility/index.js'

interface ImporterToPurge {
  modulesDir: string
  rootDir: ProjectRootDir
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
    registries: Registries
    storeDir: string
    virtualStoreDir: string
    virtualStoreDirMaxLength: number
    confirmModulesPurge?: boolean

    hoistPattern?: string[] | undefined
    forceHoistPattern?: boolean

    publicHoistPattern?: string[] | undefined
    forcePublicHoistPattern?: boolean
    global?: boolean
  }
): Promise<{ purged: boolean }> {
  const rootProject = projects.find(({ id }) => id === '.')
  if (opts.virtualStoreDirMaxLength !== modules.virtualStoreDirMaxLength) {
    if (opts.forceNewModules && (rootProject != null)) {
      globalWarn('This modules directory was created using a different virtual-store-dir-max-length value.')
      await purgeModulesDirsOfImporter(opts, rootProject)
      return { purged: true }
    }
    throw new PnpmError(
      'VIRTUAL_STORE_DIR_MAX_LENGTH_DIFF',
      'This modules directory was created using a different virtual-store-dir-max-length value.' +
      ' Run "pnpm install" to recreate the modules directory.'
    )
  }
  if (
    opts.forcePublicHoistPattern &&
    !equals(modules.publicHoistPattern ?? [], opts.publicHoistPattern ?? [])
  ) {
    if (opts.forceNewModules && (rootProject != null)) {
      globalWarn('This modules directory was created using a different public-hoist-pattern value.')
      await purgeModulesDirsOfImporter(opts, rootProject)
      return { purged: true }
    }
    throw new PnpmError(
      'PUBLIC_HOIST_PATTERN_DIFF',
      'This modules directory was created using a different public-hoist-pattern value.' +
      ' Run "pnpm install" to recreate the modules directory.'
    )
  }

  const importersToPurge: ImporterToPurge[] = []

  if (opts.forceHoistPattern && (rootProject != null)) {
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
      globalWarn(err.message)
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
      globalWarn(err.message)
      importersToPurge.push(project)
    }
  }
  if (importersToPurge.length > 0 && (rootProject == null)) {
    importersToPurge.push({
      modulesDir: path.join(opts.lockfileDir, opts.modulesDir),
      rootDir: opts.lockfileDir as ProjectRootDir,
    })
  }

  const purged = importersToPurge.length > 0
  if (purged) {
    await purgeModulesDirsOfImporters(opts, importersToPurge)
  }

  return { purged }
}

async function purgeModulesDirsOfImporter (
  opts: {
    confirmModulesPurge?: boolean
    virtualStoreDir: string
  },
  importer: ImporterToPurge
): Promise<void> {
  return purgeModulesDirsOfImporters(opts, [importer])
}

async function purgeModulesDirsOfImporters (
  opts: {
    confirmModulesPurge?: boolean
    virtualStoreDir: string
  },
  importers: ImporterToPurge[]
): Promise<void> {
  if (opts.confirmModulesPurge ?? true) {
    if (!process.stdin.isTTY) {
      throw new PnpmError('ABORTED_REMOVE_MODULES_DIR_NO_TTY', 'Aborted removal of modules directory due to no TTY', {
        hint: 'If you are running pnpm in CI, set the CI environment variable to "true".',
      })
    }
    const confirmed = await enquirer.prompt<{ question: boolean }>({
      type: 'confirm',
      name: 'question',
      message: importers.length === 1
        ? `The modules directory at "${importers[0].modulesDir}" will be removed and reinstalled from scratch. Proceed?`
        : 'The modules directories will be removed and reinstalled from scratch. Proceed?',
      initial: true,
    })
    if (!confirmed.question) {
      throw new PnpmError('ABORTED_REMOVE_MODULES_DIR', 'Aborted removal of modules directory')
    }
  }
  await Promise.all(importers.map(async (importer) => {
    logger.info({
      message: `Recreating ${importer.modulesDir}`,
      prefix: importer.rootDir,
    })
    try {
      // We don't remove the actual modules directory, just the contents of it.
      // 1. we will need the directory anyway.
      // 2. in some setups, pnpm won't even have permission to remove the modules directory.
      await removeContentsOfDir(importer.modulesDir, opts.virtualStoreDir)
    } catch (err: any) { // eslint-disable-line
      if (err.code !== 'ENOENT') throw err
    }
  }))
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
