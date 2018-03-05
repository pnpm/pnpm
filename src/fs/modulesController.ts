import loadYamlFile = require('load-yaml-file')
import path = require('path')
import writeYamlFile = require('write-yaml-file')

// The dot prefix is needed because otherwise `npm shrinkwrap`
// thinks that it is an extraneous package.
const modulesFileName = '.modules.yaml'

export const LAYOUT_VERSION = 1

export interface Modules {
  packageManager: string,
  store: string,
  skipped: string[],
  layoutVersion: number,
  independentLeaves: boolean,
  pendingBuilds: string[],
  shamefullyFlatten: boolean,
  hoistedAliases: {[pkgId: string]: string[]}
}

export async function read (modulesPath: string): Promise<Modules | null> {
  const modulesYamlPath = path.join(modulesPath, modulesFileName)
  try {
    const m = await loadYamlFile<Modules>(modulesYamlPath)
    // for backward compatibility
    // tslint:disable:no-string-literal
    if (m['storePath']) {
      m.store = m['storePath']
      delete m['storePath']
    }
    // tslint:enable:no-string-literal
    return m
  } catch (err) {
    if ((err as NodeJS.ErrnoException).code !== 'ENOENT') {
      throw err
    }
    return null
  }
}

export function save (modulesPath: string, modules: Modules) {
  const modulesYamlPath = path.join(modulesPath, modulesFileName)
  if (modules.skipped) modules.skipped.sort()
  return writeYamlFile(modulesYamlPath, modules, {sortKeys: true})
}
