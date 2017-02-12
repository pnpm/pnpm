import path = require('path')
import loadYamlFile = require('load-yaml-file')
import writeYamlFile = require('write-yaml-file')

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
    return await loadYamlFile<Graph>(graphYamlPath)
  } catch (err) {
    if ((<NodeJS.ErrnoException>err).code !== 'ENOENT') {
      throw err
    }
    return null
  }
}

export function save (modulesPath: string, graph: Graph) {
  const graphYamlPath = path.join(modulesPath, graphFileName)
  return writeYamlFile(graphYamlPath, graph, {sortKeys: true})
}
