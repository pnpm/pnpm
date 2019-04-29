import { ImporterManifest } from '@pnpm/types'
import sortKeys = require('sort-keys')
import writeJsonFile = require('write-json-file')
import writeJson5File = require('write-json5-file')
import writeYamlFile = require('write-yaml-file')

// TODO: normalize before save + preserve indent
export default function writeImporterManifest (filePath: string, manifest: ImporterManifest): Promise<void> {
  manifest = normalize(manifest)
  switch (filePath.substr(filePath.lastIndexOf('.') + 1).toLowerCase()) {
    case 'json5':
      return writeJson5File(filePath, manifest)
    case 'yaml':
      return writeYamlFile(filePath, manifest)
    case 'json':
    default:
      return writeJsonFile(filePath, manifest)
  }
}

const dependencyKeys = new Set([
  'dependencies',
  'devDependencies',
  'optionalDependencies',
  'peerDependencies',
])

function normalize (manifest: ImporterManifest) {
  const result = {}

  for (const key of Object.keys(manifest)) {
    if (!dependencyKeys.has(key)) {
      result[key] = manifest[key]
    } else if (Object.keys(manifest[key]).length !== 0) {
      result[key] = sortKeys(manifest[key])
    }
  }

  return result
}
