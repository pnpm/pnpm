import path = require('path')
import {
  read as readYaml,
  write as writeYaml
} from './yamlfs'

// The dot prefix is needed because otherwise `npm shrinkwrap`
// thinks that it is an extraneous package.
const modulesFileName = '.modules.yaml'

export type Modules = {
  storePath: string,
}

export function read (modulesPath: string): Modules | null {
  const modulesYamlPath = path.join(modulesPath, modulesFileName)
  try {
    return readYaml<Modules>(modulesYamlPath)
  } catch (err) {
    return null
  }
}

export function save (modulesPath: string, modules: Modules) {
  const modulesYamlPath = path.join(modulesPath, modulesFileName)
  writeYaml(modulesYamlPath, modules)
}
