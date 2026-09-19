import { promises as fs } from 'node:fs'
import path from 'node:path'
import util from 'node:util'

import { PnpmError } from '@pnpm/error'
import { type CommentSpecifier, insertComments } from '@pnpm/text.comments-parser'
import type { ProjectManifest } from '@pnpm/types'
import { patchDocument } from '@pnpm/yaml.document-sync'
import JSON5 from 'json5'
import writeFileAtomic from 'write-file-atomic'
import yaml from 'yaml'

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
    return writePackageYaml(filePath, manifest)
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

function stringifyJson5 (obj: object, indent: string | number, comments?: CommentSpecifier[]): string {
  const json5 = JSON5.stringify(obj, undefined, indent)
  if (comments) {
    return insertComments(json5, comments)
  }
  return json5
}

async function writePackageYaml (filePath: string, manifest: ProjectManifest): Promise<void> {
  let text: string | undefined
  try {
    text = await fs.readFile(filePath, 'utf8')
  } catch (err) {
    if (!util.types.isNativeError(err) || !('code' in err) || err.code !== 'ENOENT') throw err
  }
  const document = text == null ? new yaml.Document() : yaml.parseDocument(text)
  if (document.errors.length > 0) {
    throw new PnpmError('YAML_PARSE', `${document.errors[0].message}\nin ${filePath}`)
  }
  patchDocument(document, manifest, {
    preserveKeyOrder: true,
    preserveScalarAliases: true,
    pruneEmptyValues: false,
  })
  await fs.mkdir(path.dirname(filePath), { recursive: true })
  await writeFileAtomic(filePath, document.toString({ lineWidth: 0 }))
}
