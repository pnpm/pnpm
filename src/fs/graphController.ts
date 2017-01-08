import path = require('path')
import {
  read as readYaml,
  write as writeYaml
} from './yamlfs'

const graphFileName = '.graph.yaml'

export type Graph = {
  [name: string]: PackageGraph
}

export type PackageGraph = {
  dependents: string[],
  dependencies: DependenciesResolution
}

export type DependenciesResolution = {
  [name: string]: string
}

export async function read (modulesPath: string): Promise<Graph | null> {
  const graphYamlPath = path.join(modulesPath, graphFileName)
  try {
    return await readYaml<Graph>(graphYamlPath)
  } catch (err) {
    if ((<NodeJS.ErrnoException>err).code !== 'ENOENT') {
      throw err
    }
    return null
  }
}

export function save (modulesPath: string, graph: Graph) {
  const graphYamlPath = path.join(modulesPath, graphFileName)
  return writeYaml(graphYamlPath, graph)
}
