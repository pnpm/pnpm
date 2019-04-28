import { ImporterManifest } from '@pnpm/types'
import writeJsonFile = require('write-json-file')
import writeJson5File = require('write-json5-file')
import writeYamlFile = require('write-yaml-file')

export default function writeImporterManifest (filePath: string, manifest: ImporterManifest): Promise<void> {
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
