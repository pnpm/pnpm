import fs = require('fs')
import yaml = require('js-yaml')

export function read <T>(yamlPath: string): T {
  return yaml.safeLoad(fs.readFileSync(yamlPath, 'utf8'))
}

export function write <T>(yamlPath: string, yamlObj: T) {
  fs.writeFileSync(yamlPath, yaml.safeDump(yamlObj), 'utf8')
}
