import { promises as fs } from 'fs'
import path from 'path'
import { insertComments, type CommentSpecifier } from '@pnpm/text.comments-parser'
import { type ProjectManifest } from '@pnpm/types'
import YAML from 'yaml'
import JSON5 from 'json5'
import writeFileAtomic from 'write-file-atomic'

const YAML_FORMAT = {
  //noCompatMode: true, // TODO don't try to be compatible with older yaml versions
  //noRefs: true, // TODO don't convert duplicate objects into references
  //indent: 2,
  //indentSeq: false,
  //singleQuote: false,
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
  await fs.mkdir(path.dirname(filePath), { recursive: true })
  const fileType = filePath.slice(filePath.lastIndexOf('.') + 1).toLowerCase()
  if (fileType === 'yaml') {
    const yaml = YAML.stringify(manifest, YAML_FORMAT)
    return writeFileAtomic(filePath, yaml)
  }

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
