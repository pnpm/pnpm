import fs = require('mz/fs')
import yaml = require('js-yaml')

export async function read <T>(yamlPath: string): Promise<T> {
  const rawYaml = await fs.readFile(yamlPath, 'utf8')
  return yaml.safeLoad(rawYaml)
}

export function write <T>(yamlPath: string, yamlObj: T) {
  return fs.writeFile(yamlPath, yaml.safeDump(yamlObj), 'utf8')
}
