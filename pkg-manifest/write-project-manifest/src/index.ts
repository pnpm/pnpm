import { promises as fs } from 'fs'
import path from 'path'
import { insertJson5Comments, CommentSpecifier } from '@pnpm/text.comments-parser'
import { ProjectManifest } from '@pnpm/types'
import JSON5 from 'json5'
import writeFileAtomic from 'write-file-atomic'
import writeYamlFile from 'write-yaml-file'

const YAML_FORMAT = {
  noCompatMode: true,
  noRefs: true,
}

export async function writeProjectManifest (
  filePath: string,
  manifest: ProjectManifest,
  opts?: {
    comments?: CommentSpecifier[]
    indent?: string | number | undefined
    insertFinalNewline?: boolean
  }
): Promise<void> {
  const fileType = filePath.slice(filePath.lastIndexOf('.') + 1).toLowerCase()
  if (fileType === 'yaml') {
    return writeYamlFile(filePath, manifest, YAML_FORMAT)
  }

  await fs.mkdir(path.dirname(filePath), { recursive: true })
  const trailingNewline = opts?.insertFinalNewline === false ? '' : '\n'

  let json = (fileType === 'json5' ? JSON5 : JSON)
    .stringify(manifest, undefined, opts?.indent ?? '\t')

  if (fileType === 'json5' && opts?.comments) {
    json = insertJson5Comments(json, opts.comments)
  }

  return writeFileAtomic(filePath, `${json}${trailingNewline}`)
}
