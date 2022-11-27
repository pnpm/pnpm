import { promises as fs } from 'fs'
import path from 'path'
import { insertComments, CommentSpecifier } from '@pnpm/text.comments-parser'
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
  const indent = opts?.indent ?? '\t'

  const json = (
    fileType === 'json5'
      ? stringifyJson5(manifest, indent, opts?.comments)
      : JSON.stringify(manifest, undefined, indent)
  )

  return writeFileAtomic(filePath, `${json}${trailingNewline}`)
}

function stringifyJson5 (obj: object, indent: string | number, comments?: CommentSpecifier[]) {
  const json5 = JSON5.stringify(obj, undefined, indent)
  if (comments) {
    return insertComments(json5, comments)
  }
  return json5
}
