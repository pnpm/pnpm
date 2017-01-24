import path = require('path')
import {
  read as readYaml,
  write as writeYaml
} from './yamlfs'

// The dot prefix is needed because otherwise `npm shrinkwrap`
// thinks that it is an extraneous package.
const modulesFileName = '.modules.yaml'

export type Modules = {
  packageManager: string,
  storePath: string,
}

export async function read (modulesPath: string): Promise<Modules | null> {
  const modulesYamlPath = path.join(modulesPath, modulesFileName)
  try {
    return await readYaml<Modules>(modulesYamlPath)
  } catch (err) {
    if ((<NodeJS.ErrnoException>err).code !== 'ENOENT') {
      throw err
    }
    return null
  }
}

export function save (modulesPath: string, modules: Modules) {
  const modulesYamlPath = path.join(modulesPath, modulesFileName)
  return writeYaml(modulesYamlPath, modules)
}
