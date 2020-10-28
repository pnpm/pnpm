import { ProjectManifest } from '@pnpm/types'
import path = require('path')
import JSON5 = require('json5')
import fs = require('mz/fs')
import writeFileAtomic = require('write-file-atomic')
import writeYamlFile = require('write-yaml-file')

const YAML_FORMAT = {
  noCompatMode: true,
  noRefs: true,
}

export default async function writeProjectManifest (
  filePath: string,
  manifest: ProjectManifest,
  opts?: {
    indent?: string | number | undefined
    insertFinalNewline?: boolean
  }
): Promise<void> {
  const fileType = filePath.substr(filePath.lastIndexOf('.') + 1).toLowerCase()
  if (fileType === 'yaml') {
    return writeYamlFile(filePath, manifest, YAML_FORMAT)
  }

  await fs.mkdir(path.dirname(filePath), { recursive: true })
  const trailingNewline = opts?.insertFinalNewline === false ? '' : '\n'

  const json = (fileType === 'json5' ? JSON5 : JSON)
    .stringify(manifest, null, opts?.indent ?? '\t')

  return writeFileAtomic(filePath, `${json}${trailingNewline}`)
}
