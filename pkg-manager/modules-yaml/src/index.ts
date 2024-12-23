import path from 'path'
import { type DepPath, type DependenciesField, type HoistedDependencies, type Registries } from '@pnpm/types'
import readYamlFile from 'read-yaml-file'
import mapValues from 'ramda/src/map'
import isWindows from 'is-windows'
import writeYamlFile from 'write-yaml-file'

// The dot prefix is needed because otherwise `npm shrinkwrap`
// thinks that it is an extraneous package.
const MODULES_FILENAME = '.modules.yaml'

export type IncludedDependencies = {
  [dependenciesField in DependenciesField]: boolean
}

export interface Modules {
  hoistedAliases?: { [depPath: DepPath]: string[] } // for backward compatibility
  hoistedDependencies: HoistedDependencies
  hoistPattern?: string[]
  included: IncludedDependencies
  layoutVersion: number
  nodeLinker?: 'hoisted' | 'isolated' | 'pnp'
  packageManager: string
  pendingBuilds: string[]
  ignoredBuilds?: string[]
  prunedAt: string
  registries?: Registries // nullable for backward compatibility
  shamefullyHoist?: boolean // for backward compatibility
  publicHoistPattern?: string[]
  skipped: string[]
  storeDir: string
  virtualStoreDir: string
  virtualStoreDirMaxLength: number
  injectedDeps?: Record<string, string[]>
  hoistedLocations?: Record<string, string[]>
}

export async function readModulesManifest (modulesDir: string): Promise<Modules | null> {
  const modulesYamlPath = path.join(modulesDir, MODULES_FILENAME)
  let modules!: Modules
  try {
    modules = await readYamlFile<Modules>(modulesYamlPath)
    if (!modules) return modules
  } catch (err: any) { // eslint-disable-line
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
    if (modules.publicHoistPattern == null) {
      modules.publicHoistPattern = ['*']
    }
    if ((modules.hoistedAliases != null) && !modules.hoistedDependencies) {
      modules.hoistedDependencies = mapValues(
        (aliases) => Object.fromEntries(aliases.map((alias) => [alias, 'public'])),
        modules.hoistedAliases
      )
    }
    break
  case false:
    if (modules.publicHoistPattern == null) {
      modules.publicHoistPattern = []
    }
    if ((modules.hoistedAliases != null) && !modules.hoistedDependencies) {
      modules.hoistedDependencies = {}
      for (const depPath of Object.keys(modules.hoistedAliases)) {
        modules.hoistedDependencies[depPath as DepPath] = {}
        for (const alias of modules.hoistedAliases[depPath as DepPath]) {
          modules.hoistedDependencies[depPath as DepPath][alias] = 'private'
        }
      }
    }
    break
  }
  if (!modules.prunedAt) {
    modules.prunedAt = new Date().toUTCString()
  }
  if (!modules.virtualStoreDirMaxLength) {
    modules.virtualStoreDirMaxLength = 120
  }
  return modules
}

const YAML_OPTS = {
  lineWidth: 1000,
  noCompatMode: true,
  noRefs: true,
  sortKeys: true,
}

export async function writeModulesManifest (
  modulesDir: string,
  modules: Modules & { registries: Registries },
  opts?: {
    makeModulesDir?: boolean
  }
): Promise<void> {
  const modulesYamlPath = path.join(modulesDir, MODULES_FILENAME)
  const saveModules = { ...modules }
  if (saveModules.skipped) saveModules.skipped.sort()

  if (saveModules.hoistPattern == null || (saveModules.hoistPattern as unknown) === '') {
    // Because the YAML writer fails on undefined fields
    delete saveModules.hoistPattern
  }
  if (saveModules.publicHoistPattern == null) {
    delete saveModules.publicHoistPattern
  }
  if ((saveModules.hoistedAliases == null) || (saveModules.hoistPattern == null) && (saveModules.publicHoistPattern == null)) {
    delete saveModules.hoistedAliases
  }
  // We should store the absolute virtual store directory path on Windows
  // because junctions are used on Windows. Junctions will break even if
  // the relative path to the virtual store remains the same after moving
  // a project.
  if (!isWindows()) {
    saveModules.virtualStoreDir = path.relative(modulesDir, saveModules.virtualStoreDir)
  }
  try {
    await writeYamlFile(modulesYamlPath, saveModules, {
      ...YAML_OPTS,
      makeDir: opts?.makeModulesDir ?? false,
    })
  } catch (err: any) { // eslint-disable-line
    if ((err as NodeJS.ErrnoException).code !== 'ENOENT') throw err
  }
}
