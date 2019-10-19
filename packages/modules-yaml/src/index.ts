import { DependenciesField, Registries } from '@pnpm/types'
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
  independentLeaves: boolean,
  layoutVersion: number,
  packageManager: string,
  pendingBuilds: string[],
  registries?: Registries, // nullable for backward compatibility
  shamefullyHoist: boolean,
  skipped: string[],
  store: string,
  virtualStoreDir: string,
}

export async function read (modulesDir: string): Promise<Modules | null> {
  const modulesYamlPath = path.join(modulesDir, MODULES_FILENAME)
  try {
    const modules = await readYamlFile<Modules>(modulesYamlPath)
    if (!modules.virtualStoreDir) {
      modules.virtualStoreDir = path.join(modulesDir, '.pnpm')
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
  modules: Modules & { registries: Registries },
) {
  const modulesYamlPath = path.join(modulesDir, MODULES_FILENAME)
  if (modules.skipped) modules.skipped.sort()

  if (!modules.hoistPattern) {
    // Because the YAML writer fails on undefined fields
    delete modules.hoistPattern
    delete modules.hoistedAliases
  }
  return writeYamlFile(modulesYamlPath, modules, YAML_OPTS)
}
