import { constants } from 'node:fs'
import { type FileHandle, lstat, open, readFile } from 'node:fs/promises'
import { StringDecoder } from 'node:string_decoder'
import util from 'node:util'

import { PnpmError } from '@pnpm/error'
import stripBom from 'strip-bom'

export const YAML_DOCUMENT_SEPARATOR = '\n---\n'
export const YAML_DOCUMENT_START = '---\n'

const READ_BUFFER_SIZE = 64 * 1024
const LOCKFILE_READ_FLAGS = constants.O_RDONLY | (process.platform === 'win32' ? 0 : constants.O_NOFOLLOW)

/**
 * Reads the first YAML document from a multi-document YAML file using streaming.
 * The file must start with "---\n" to indicate it contains an env lockfile document.
 * Stops reading as soon as the second document separator is found.
 * Returns null if the file doesn't exist or doesn't start with "---\n".
 */
export async function streamReadFirstYamlDocument (filePath: string, readBufferSize = READ_BUFFER_SIZE): Promise<string | null> {
  let fileHandle: FileHandle | undefined
  let buffer = ''
  let firstChunk = true
  try {
    fileHandle = await open(filePath, constants.O_RDONLY)
    const decoder = new StringDecoder('utf8')
    const readBuffer = Buffer.allocUnsafe(normalizeReadBufferSize(readBufferSize))
    let position = 0
    while (true) {
      const { bytesRead } = await fileHandle.read(readBuffer, 0, readBuffer.length, position) // eslint-disable-line no-await-in-loop
      if (bytesRead === 0) break
      position += bytesRead
      let chunk = decoder.write(readBuffer.subarray(0, bytesRead))
      if (firstChunk && chunk.length > 0) {
        // Strip BOM from the first chunk. Safe because the decoder uses utf8,
        // so the 3-byte BOM is decoded into a single \uFEFF character.
        chunk = stripBom(chunk)
        firstChunk = false
      }
      buffer += chunk
      // Normalize CRLF (Windows) to LF so document separator detection works.
      buffer = buffer.replace(/\r\n/g, '\n')
      if (canRejectDocumentStart(buffer)) {
        return null
      }
      const sep = buffer.indexOf(YAML_DOCUMENT_SEPARATOR, YAML_DOCUMENT_START.length)
      if (sep !== -1) {
        return buffer.slice(YAML_DOCUMENT_START.length, sep)
      }
    }
    const remainder = decoder.end()
    if (remainder.length > 0) {
      buffer += firstChunk ? stripBom(remainder) : remainder
      buffer = buffer.replace(/\r\n/g, '\n')
    }
    return null
  } catch (err: unknown) {
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') {
      return null
    }
    throw err
  } finally {
    await fileHandle?.close().catch(() => {})
  }
}

export async function readLockfileToString (filePath: string): Promise<string | null> {
  try {
    return stripBom(await readFile(filePath, 'utf8')).replace(/\r\n/g, '\n')
  } catch (err: unknown) {
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') {
      return null
    }
    throw err
  }
}

export async function readLockfileToStringNoFollow (filePath: string): Promise<string | null> {
  let fileHandle: FileHandle | undefined
  try {
    fileHandle = await openLockfileNoFollow(filePath)
    return await fileHandle.readFile('utf8')
  } catch (err: unknown) {
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') {
      return null
    }
    throw err
  } finally {
    await fileHandle?.close().catch(() => {})
  }
}

async function openLockfileNoFollow (filePath: string): Promise<FileHandle> {
  await ensureLockfileIsNotSymlink(filePath)
  try {
    return await open(filePath, LOCKFILE_READ_FLAGS)
  } catch (err: unknown) {
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ELOOP') {
      throw symlinkedLockfileError(filePath)
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
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') {
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
  if (sep === -1) return null
  return content.slice(YAML_DOCUMENT_START.length, sep)
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
