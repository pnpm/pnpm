import path = require('path')
import loadYamlFile = require('load-yaml-file')
import writeYamlFile = require('write-yaml-file')

// The dot prefix is needed because otherwise `npm shrinkwrap`
// thinks that it is an extraneous package.
const modulesFileName = '.modules.yaml'

export const LAYOUT_VERSION = 1

export type Modules = {
  packageManager: string,
  storePath: string,
  skipped: string[],
  layoutVersion: number,
  independentLeaves: boolean,
}

export async function read (modulesPath: string): Promise<Modules | null> {
  const modulesYamlPath = path.join(modulesPath, modulesFileName)
  try {
    return await loadYamlFile<Modules>(modulesYamlPath)
  } catch (err) {
    if ((<NodeJS.ErrnoException>err).code !== 'ENOENT') {
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
