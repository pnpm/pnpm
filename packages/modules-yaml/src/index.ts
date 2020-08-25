import { DependenciesField, HoistedDependencies, Registries } from '@pnpm/types'
import readYamlFile from 'read-yaml-file'
import path = require('path')
import isWindows = require('is-windows')
import writeYamlFile = require('write-yaml-file')

// The dot prefix is needed because otherwise `npm shrinkwrap`
// thinks that it is an extraneous package.
const MODULES_FILENAME = '.modules.yaml'

export type IncludedDependencies = {
  [dependenciesField in DependenciesField]: boolean
}

export interface Modules {
  hoistedAliases?: {[depPath: string]: string[]} // for backward compatibility
  hoistedDependencies: HoistedDependencies
  hoistPattern?: string[]
  included: IncludedDependencies
  layoutVersion: number
  packageManager: string
  pendingBuilds: string[]
  registries?: Registries // nullable for backward compatibility
  shamefullyHoist?: boolean // for backward compatibility
  publicHoistPattern?: string[]
  skipped: string[]
  storeDir: string
  virtualStoreDir: string
}

export async function read (modulesDir: string): Promise<Modules | null> {
  const modulesYamlPath = path.join(modulesDir, MODULES_FILENAME)
  let modules!: Modules
  try {
    modules = await readYamlFile<Modules>(modulesYamlPath)
  } catch (err) {
    if ((err as NodeJS.ErrnoException).code !== 'ENOENT') {
      throw err
    }
    return null
  }
  if (!modules.virtualStoreDir) {
    modules.virtualStoreDir = path.join(modulesDir, '.pnpm')
  } else if (!path.isAbsolute(modules.virtualStoreDir)) {
    modules.virtualStoreDir = path.join(modulesDir, modules.virtualStoreDir)
  }
  switch (modules.shamefullyHoist) {
  case true:
    if (!modules.publicHoistPattern) {
      modules.publicHoistPattern = ['*']
    }
    if (modules.hoistedAliases && !modules.hoistedDependencies) {
      modules.hoistedDependencies = {}
      for (const depPath of Object.keys(modules.hoistedAliases)) {
        modules.hoistedDependencies[depPath] = {}
        for (const alias of modules.hoistedAliases[depPath]) {
          modules.hoistedDependencies[depPath][alias] = 'public'
        }
      }
    }
    break
  case false:
    if (!modules.publicHoistPattern) {
      modules.publicHoistPattern = []
    }
    if (modules.hoistedAliases && !modules.hoistedDependencies) {
      modules.hoistedDependencies = {}
      for (const depPath of Object.keys(modules.hoistedAliases)) {
        modules.hoistedDependencies[depPath] = {}
        for (const alias of modules.hoistedAliases[depPath]) {
          modules.hoistedDependencies[depPath][alias] = 'private'
        }
      }
    }
    break
  }
  return modules
}

const YAML_OPTS = {
  noCompatMode: true,
  noRefs: true,
  sortKeys: true,
}

export function write (
  modulesDir: string,
  modules: Modules & { registries: Registries }
) {
  const modulesYamlPath = path.join(modulesDir, MODULES_FILENAME)
  const saveModules = { ...modules }
  if (saveModules.skipped) saveModules.skipped.sort()

  if (!saveModules.hoistPattern) {
    // Because the YAML writer fails on undefined fields
    delete saveModules.hoistPattern
  }
  if (!saveModules.publicHoistPattern) {
    delete saveModules.publicHoistPattern
  }
  if (!saveModules.hoistedAliases || !saveModules.hoistPattern && !saveModules.publicHoistPattern) {
    delete saveModules.hoistedAliases
  }
  // We should store the absolute virtual store directory path on Windows
  // because junctions are used on Windows. Junctions will break even if
  // the relative path to the virtual store remains the same after moving
  // a project.
  if (!isWindows()) {
    saveModules.virtualStoreDir = path.relative(modulesDir, saveModules.virtualStoreDir)
  }
  return writeYamlFile(modulesYamlPath, saveModules, YAML_OPTS)
}
