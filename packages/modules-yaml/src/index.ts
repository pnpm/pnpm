import { DependenciesField, Registries } from '@pnpm/types'
import isWindows = require('is-windows')
import path = require('path')
import readYamlFile from 'read-yaml-file'
import writeYamlFile = require('write-yaml-file')

// The dot prefix is needed because otherwise `npm shrinkwrap`
// thinks that it is an extraneous package.
const MODULES_FILENAME = '.modules.yaml'

export type IncludedDependencies = {
  [dependenciesField in DependenciesField]: boolean
}

export interface Modules {
  hoistedAliases: {[depPath: string]: string[]}
  hoistPattern?: string[]
  included: IncludedDependencies,
  layoutVersion: number,
  packageManager: string,
  pendingBuilds: string[],
  registries?: Registries, // nullable for backward compatibility
  shamefullyHoist: boolean,
  skipped: string[],
  storeDir: string,
  virtualStoreDir: string,
}

export async function read (modulesDir: string): Promise<Modules | null> {
  const modulesYamlPath = path.join(modulesDir, MODULES_FILENAME)
  try {
    const modules = await readYamlFile<Modules>(modulesYamlPath)
    if (!modules.virtualStoreDir) {
      modules.virtualStoreDir = path.join(modulesDir, '.pnpm')
    } else if (!path.isAbsolute(modules.virtualStoreDir)) {
      modules.virtualStoreDir = path.join(modulesDir, modules.virtualStoreDir)
    }
    return modules
  } catch (err) {
    if ((err as NodeJS.ErrnoException).code !== 'ENOENT') {
      throw err
    }
    return null
  }
}

const YAML_OPTS = { sortKeys: true }

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
