import { ProjectManifest } from '@pnpm/types'
import writeJsonFile = require('write-json-file')
import writeJson5File = require('write-json5-file')
import writeYamlFile = require('write-yaml-file')

const YAML_FORMAT = {
  noCompatMode: true,
  noRefs: true,
}

export default function writeProjectManifest (
  filePath: string,
  manifest: ProjectManifest,
  opts?: { indent?: string | number | undefined }
): Promise<void> {
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
