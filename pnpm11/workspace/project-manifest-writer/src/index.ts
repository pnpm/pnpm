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
    crlf?: boolean
    indent?: string | number | undefined
    insertFinalNewline?: boolean
  }
): Promise<void> {
  const fileType = filePath.slice(filePath.lastIndexOf('.') + 1).toLowerCase()
  if (fileType === 'yaml') {
    return writePackageYaml(filePath, manifest, opts?.crlf)
  }

  await fs.mkdir(path.dirname(filePath), { recursive: true })
  const crlf = opts?.crlf ?? (await readFileIfExists(filePath))?.includes('\r\n') ?? false
  const newline = crlf ? '\r\n' : '\n'
  const trailingNewline = opts?.insertFinalNewline === false ? '' : newline
  const indent = opts?.indent ?? '\t'

  let json = (
    fileType === 'json5'
      ? stringifyJson5(manifest, indent, opts?.comments)
      : JSON.stringify(manifest, undefined, indent)
  )

  if (crlf) {
    json = json.replace(/\r?\n/g, '\r\n')
  }

  return writeFileAtomic(filePath, `${json}${trailingNewline}`)
}

function stringifyJson5 (obj: object, indent: string | number, comments?: CommentSpecifier[]): string {
  const json5 = JSON5.stringify(obj, undefined, indent)
  if (comments) {
    return insertComments(json5, comments)
  }
  return json5
}

async function writePackageYaml (filePath: string, manifest: ProjectManifest, crlf?: boolean): Promise<void> {
  const text = await readFileIfExists(filePath)
  const document = text == null ? new yaml.Document() : yaml.parseDocument(text)
  if (document.errors.length > 0) {
    throw new PnpmError('YAML_PARSE', `${document.errors[0].message}\nin ${filePath}`)
  }
  patchDocument(document, manifest, {
    stringifyKey: String,
    preserveKeyOrder: true,
    preserveScalarAliases: true,
    pruneEmptyValues: false,
  })
  await fs.mkdir(path.dirname(filePath), { recursive: true })
  let content = document.toString({ lineWidth: 0 })
  if (crlf ?? text?.includes('\r\n')) {
    content = content.replace(/\r?\n/g, '\r\n')
  }
  await writeFileAtomic(filePath, content)
}

async function readFileIfExists (filePath: string): Promise<string | undefined> {
  try {
    return await fs.readFile(filePath, 'utf8')
  } catch (err) {
    if (!util.types.isNativeError(err) || !('code' in err) || err.code !== 'ENOENT') throw err
  }
  return undefined
}
