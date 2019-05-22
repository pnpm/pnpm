import { ImporterManifest } from '@pnpm/types'
import sortKeys = require('sort-keys')
import writeJsonFile = require('write-json-file')
import writeJson5File = require('write-json5-file')
import writeYamlFile = require('write-yaml-file')

const YAML_FORMAT = {
  noCompatMode: true,
  noRefs: true,
}

export default function writeImporterManifest (
  filePath: string,
  manifest: ImporterManifest,
  opts?: { indent?: string | number | undefined },
): Promise<void> {
  manifest = normalize(manifest)
  switch (filePath.substr(filePath.lastIndexOf('.') + 1).toLowerCase()) {
    case 'json5':
      return writeJson5File(filePath, manifest, opts)
    case 'yaml':
      return writeYamlFile(filePath, manifest, YAML_FORMAT)
    case 'json':
    default:
      return writeJsonFile(filePath, manifest, opts)
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
