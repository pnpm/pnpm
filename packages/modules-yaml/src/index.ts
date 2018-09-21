import loadYamlFile = require('load-yaml-file')
import path = require('path')
import writeYamlFile = require('write-yaml-file')

// The dot prefix is needed because otherwise `npm shrinkwrap`
// thinks that it is an extraneous package.
const modulesFileName = '.modules.yaml'

export interface Modules {
  hoistedAliases: {[depPath: string]: string[]}
  independentLeaves: boolean,
  layoutVersion: number,
  packageManager: string,
  pendingBuilds: string[],
  shamefullyFlatten: boolean,
  skipped: string[],
  store: string,
}

type ModulesContent = {
  nodeModulesType: 'shared',
  independentLeaves: boolean,
  layoutVersion: number,
  packageManager: string,
  pendingBuilds: string[],
  skipped: string[],
  store: string,
} | {
  nodeModulesType: 'dedicated',
  hoistedAliases: {[depPath: string]: string[]}
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

export async function read (
  sharedNodeModulesPath: string,
  proxyNodeModulesPath: string,
): Promise<Modules | null> {
  const proxyModulesYamlPath = path.join(proxyNodeModulesPath, modulesFileName)
  let m!: ModulesContent
  try {
    m = await loadYamlFile<ModulesContent>(proxyModulesYamlPath)

    if (m.nodeModulesType === 'proxy') {
      const sharedModulesYamlPath = path.join(m.shrinkwrapDirectory, 'node_modules', modulesFileName)
      return {
        hoistedAliases: m.hoistedAliases,
        shamefullyFlatten: m.shamefullyFlatten,
        ...await loadYamlFile(sharedModulesYamlPath),
      } as Modules
    }
  } catch (err) {
    if ((err as NodeJS.ErrnoException).code !== 'ENOENT') {
      throw err
    }
    if (sharedNodeModulesPath === proxyNodeModulesPath) {
      return null
    }

    try {
      const sharedModulesYamlPath = path.join(sharedNodeModulesPath, modulesFileName)
      m = await loadYamlFile<ModulesContent>(sharedModulesYamlPath)
    } catch (err) {
      if ((err as NodeJS.ErrnoException).code !== 'ENOENT') {
        throw err
      }
      return null
    }
  }

  // for backward compatibility
  // tslint:disable:no-string-literal
  if (m['storePath']) {
    m['store'] = m['storePath']
    delete m['storePath']
  }
  // tslint:enable:no-string-literal
  return m as Modules
}

const YAML_OPTS = {sortKeys: true}

export function write (
  sharedNodeModulesPath: string,
  proxyNodeModulesPath: string,
  modules: Modules,
) {
  if (modules['skipped']) modules['skipped'].sort() // tslint:disable-line:no-string-literal
  if (sharedNodeModulesPath === proxyNodeModulesPath) {
    const modulesYamlPath = path.join(proxyNodeModulesPath, modulesFileName)
    return writeYamlFile(modulesYamlPath, {
      ...modules,
      nodeModulesType: 'dedicated',
    }, YAML_OPTS)
  }
  const sharedModulesYamlPath = path.join(sharedNodeModulesPath, modulesFileName)
  const sharedModules = {
    independentLeaves: modules.independentLeaves,
    layoutVersion: modules.layoutVersion,
    nodeModulesType: 'shared',
    packageManager: modules.packageManager,
    pendingBuilds: modules.pendingBuilds,
    skipped: modules.skipped,
    store: modules.store,
  }

  const proxyModulesYamlPath = path.join(proxyNodeModulesPath, modulesFileName)
  const proxyModules = {
    hoistedAliases: modules.hoistedAliases,
    nodeModulesType: 'proxy',
    shamefullyFlatten: modules.shamefullyFlatten,
    shrinkwrapDirectory: sharedNodeModulesPath,
  }

  return Promise.all([
    writeYamlFile(sharedModulesYamlPath, sharedModules, YAML_OPTS),
    writeYamlFile(proxyModulesYamlPath, proxyModules, YAML_OPTS),
  ])
}
