import fs = require('mz/fs')
import yaml = require('js-yaml')

export async function read <T>(yamlPath: string): Promise<T> {
  const rawYaml = await fs.readFile(yamlPath, 'utf8')
  return yaml.safeLoad(rawYaml)
}

export function write <T>(yamlPath: string, yamlObj: T) {
  const rawYaml = yaml.safeDump(yamlObj, <Object>{sortKeys: true})
  return fs.writeFile(yamlPath, rawYaml, 'utf8')
}
