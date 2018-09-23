import { DependenciesField } from '@pnpm/types'
import loadYamlFile = require('load-yaml-file')
import path = require('path')
import writeYamlFile = require('write-yaml-file')

// The dot prefix is needed because otherwise `npm shrinkwrap`
// thinks that it is an extraneous package.
const MODULES_FILENAME = '.modules.yaml'

export type IncludedDependencies = {
  [dependenciesField in DependenciesField]: boolean
}

export interface Modules {
  hoistedAliases: {[depPath: string]: string[]}
  included: IncludedDependencies,
  independentLeaves: boolean,
  layoutVersion: number,
  packageManager: string,
  pendingBuilds: string[],
  shamefullyFlatten: boolean,
  shrinkwrapDirectory?: string,
  skipped: string[],
  store: string,
}

type ModulesContent = {
  nodeModulesType: 'shared',
  included: IncludedDependencies,
  independentLeaves: boolean,
  layoutVersion: number,
  packageManager: string,
  pendingBuilds: string[],
  skipped: string[],
  store: string,
} | {
  nodeModulesType: 'dedicated',
  hoistedAliases: {[depPath: string]: string[]}
  included: IncludedDependencies,
  independentLeaves: boolean,
  layoutVersion: number,
  packageManager: string,
  pendingBuilds: string[],
  shamefullyFlatten: boolean,
  skipped: string[],
  store: string,
} | {
  nodeModulesType: 'proxy',
  hoistedAliases: {[depPath: string]: string[]}
  shamefullyFlatten: boolean,
  shrinkwrapDirectory: string,
}

export async function read (nodeModulesPath: string): Promise<Modules | null> {
  const proxyModulesYamlPath = path.join(nodeModulesPath, MODULES_FILENAME)
  let m!: ModulesContent
  try {
    m = await loadYamlFile<ModulesContent>(proxyModulesYamlPath)

    switch (m.nodeModulesType) {
      case 'proxy':
        const sharedModulesYamlPath = path.join(m.shrinkwrapDirectory, 'node_modules', MODULES_FILENAME)
        return {
          hoistedAliases: m.hoistedAliases,
          shamefullyFlatten: m.shamefullyFlatten,
          shrinkwrapDirectory: m.shrinkwrapDirectory,
          ...await loadYamlFile<object>(sharedModulesYamlPath),
        } as Modules
      case 'shared':
        return {
          hoistedAliases: {},
          shamefullyFlatten: false,
          ...m,
        } as Modules
      case 'dedicated':
      default:
        return m
    }
  } catch (err) {
    if ((err as NodeJS.ErrnoException).code !== 'ENOENT') {
      throw err
    }
    return null
  }
}

const YAML_OPTS = {sortKeys: true}

export function write (
  sharedNodeModulesPath: string,
  proxyNodeModulesPath: string,
  modules: Modules,
) {
  if (modules['skipped']) modules['skipped'].sort() // tslint:disable-line:no-string-literal
  if (sharedNodeModulesPath === proxyNodeModulesPath) {
    const modulesYamlPath = path.join(proxyNodeModulesPath, MODULES_FILENAME)
    return writeYamlFile(modulesYamlPath, {
      ...modules,
      nodeModulesType: 'dedicated',
    }, YAML_OPTS)
  }
  const sharedModulesYamlPath = path.join(sharedNodeModulesPath, MODULES_FILENAME)
  const sharedModules = {
    included: modules.included,
    independentLeaves: modules.independentLeaves,
    layoutVersion: modules.layoutVersion,
    nodeModulesType: 'shared',
    packageManager: modules.packageManager,
    pendingBuilds: modules.pendingBuilds,
    skipped: modules.skipped,
    store: modules.store,
  }

  const proxyModulesYamlPath = path.join(proxyNodeModulesPath, MODULES_FILENAME)
  const proxyModules = {
    hoistedAliases: modules.hoistedAliases,
    nodeModulesType: 'proxy',
    shamefullyFlatten: modules.shamefullyFlatten,
    shrinkwrapDirectory: path.dirname(sharedNodeModulesPath),
  }

  return Promise.all([
    writeYamlFile(sharedModulesYamlPath, sharedModules, YAML_OPTS),
    writeYamlFile(proxyModulesYamlPath, proxyModules, YAML_OPTS),
  ])
}
