import { constants } from 'node:fs'
import { type FileHandle, lstat, open, readFile } from 'node:fs/promises'
import { StringDecoder } from 'node:string_decoder'

import { isError, PnpmError } from '@pnpm/error'
import stripBom from 'strip-bom'

export const YAML_DOCUMENT_SEPARATOR = '\n---\n'
export const YAML_DOCUMENT_START = '---\n'

const READ_BUFFER_SIZE = 64 * 1024

/** How an env document lays out the start of its root importer. */
const ROOT_IMPORTER_CONFIG_DEPENDENCIES = '\n  .:\n    configDependencies:'

/**
 * Reads the first YAML document from a multi-document YAML file using streaming.
 * The file must start with "---\n" to indicate it contains an env lockfile document.
 * Stops reading as soon as the second document separator is found.
 * A file with no separator is env-only, see {@link envOnlyDocument}.
 * Returns null if the file doesn't exist or doesn't start with "---\n".
 */
export async function streamReadFirstYamlDocument (filePath: string, readBufferSize = READ_BUFFER_SIZE): Promise<string | null> {
  let fileHandle: FileHandle | undefined
  try {
    fileHandle = await open(filePath, constants.O_RDONLY)
    return await readFirstYamlDocument(fileHandle, readBufferSize)
  } catch (err: unknown) {
    if (isError(err) && 'code' in err && err.code === 'ENOENT') {
      return null
    }
    throw err
  } finally {
    await fileHandle?.close().catch(() => {})
  }
}

interface DecodedText {
  text: string
  bomStripped: boolean
}

async function readFirstYamlDocument (fileHandle: FileHandle, readBufferSize: number): Promise<string | null> {
  const decoder = new StringDecoder('utf8')
  const readBuffer = Buffer.allocUnsafe(normalizeReadBufferSize(readBufferSize))
  const decoded: DecodedText = { text: '', bomStripped: false }
  let position = 0
  while (true) {
    const { bytesRead } = await fileHandle.read(readBuffer, 0, readBuffer.length, position) // eslint-disable-line no-await-in-loop -- each read continues at the position the previous one reached
    if (bytesRead === 0) return documentWithoutSeparator(decoded.text + decoder.end())
    position += bytesRead
    appendDecodedChunk(decoded, decoder.write(readBuffer.subarray(0, bytesRead)))
    const document = findFirstYamlDocument(decoded.text)
    if (document !== undefined) return document
  }
}

function appendDecodedChunk (decoded: DecodedText, chunk: string): void {
  if (!decoded.bomStripped && chunk.length > 0) {
    // Strip BOM from the first chunk. Safe because the decoder uses utf8,
    // so the 3-byte BOM is decoded into a single \uFEFF character.
    chunk = stripBom(chunk)
    decoded.bomStripped = true
  }
  // Normalize CRLF (Windows) to LF so document separator detection works.
  decoded.text = (decoded.text + chunk).replace(/\r\n/g, '\n')
}

function documentWithoutSeparator (text: string): string | null {
  return text.startsWith(YAML_DOCUMENT_START) ? envOnlyDocument(text.slice(YAML_DOCUMENT_START.length)) : null
}

/**
 * The first document once `text` holds all of it, `null` once `text` cannot start an env
 * document, and `undefined` while more of the file is needed to tell.
 */
function findFirstYamlDocument (text: string): string | null | undefined {
  if (canRejectDocumentStart(text)) return null
  const sep = text.indexOf(YAML_DOCUMENT_SEPARATOR, YAML_DOCUMENT_START.length)
  return sep === -1 ? undefined : text.slice(YAML_DOCUMENT_START.length, sep)
}

export async function readLockfileToString (filePath: string): Promise<string | null> {
  try {
    return stripBom(await readFile(filePath, 'utf8')).replace(/\r\n/g, '\n')
  } catch (err: unknown) {
    if (isError(err) && 'code' in err && err.code === 'ENOENT') {
      return null
    }
    throw err
  }
}

/**
 * Refuses a symlinked lockfile before a write, which would land on the link's
 * target — any file the user can write. Reads may follow it: sandboxes stage
 * `pnpm-lock.yaml` as a symlink, and lockfile content is untrusted either way
 * (https://github.com/pnpm/pnpm/issues/13073).
 */
export async function ensureLockfileIsNotSymlink (filePath: string): Promise<void> {
  let stat
  try {
    stat = await lstat(filePath)
  } catch (err: unknown) {
    if (isError(err) && 'code' in err && err.code === 'ENOENT') {
      return
    }
    throw err
  }
  if (stat.isSymbolicLink()) {
    throw symlinkedLockfileError(filePath)
  }
}

function symlinkedLockfileError (filePath: string): PnpmError {
  return new PnpmError('LOCKFILE_IS_SYMLINK', `Refusing to write symlinked lockfile at ${filePath}`)
}

function canRejectDocumentStart (buffer: string): boolean {
  if (buffer.length < YAML_DOCUMENT_START.length) return false
  if (buffer === '---\r') return false
  return !buffer.startsWith(YAML_DOCUMENT_START)
}

function normalizeReadBufferSize (readBufferSize: number): number {
  const size = Number.isFinite(readBufferSize) ? Math.floor(readBufferSize) : READ_BUFFER_SIZE
  return size > 0 ? size : READ_BUFFER_SIZE
}

/** The in-memory counterpart of {@link streamReadFirstYamlDocument}. */
export function extractEnvDocument (content: string): string | null {
  content = content.replace(/\r\n/g, '\n')
  if (!content.startsWith(YAML_DOCUMENT_START)) return null
  const sep = content.indexOf(YAML_DOCUMENT_SEPARATOR, YAML_DOCUMENT_START.length)
  if (sep === -1) return envOnlyDocument(content.slice(YAML_DOCUMENT_START.length))
  return content.slice(YAML_DOCUMENT_START.length, sep)
}

/**
 * The env document of a lockfile whose empty main document was trimmed off
 * together with the separator: `rest` up to where the separator would start, so
 * a closing `---` line that has no newline after it is dropped too. Only a body
 * whose root importer opens with the `configDependencies` key every env
 * document writes qualifies, so a main lockfile that merely opens with `---`
 * is not mistaken for one.
 */
function envOnlyDocument (rest: string): string | null {
  let document = rest
  for (const end of ['\n---', '\n']) {
    if (rest.endsWith(end)) {
      document = rest.slice(0, -end.length)
      break
    }
  }
  return document.includes(ROOT_IMPORTER_CONFIG_DEPENDENCIES) ? document : null
}

/**
 * Extracts the main lockfile content (second YAML document) from a combined string.
 * If the file starts with "---\n", returns the content after the separator.
 * If there is no separator, returns empty string (file is env-only).
 * Otherwise returns the entire content (no env document present).
 */
export function extractMainDocument (content: string): string {
  content = content.replace(/\r\n/g, '\n')
  if (!content.startsWith(YAML_DOCUMENT_START)) return content
  const sep = content.indexOf(YAML_DOCUMENT_SEPARATOR, YAML_DOCUMENT_START.length)
  if (sep === -1) return ''
  return content.slice(sep + YAML_DOCUMENT_SEPARATOR.length)
}
