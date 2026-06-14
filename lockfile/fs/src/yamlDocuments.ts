import { createReadStream, type ReadStream } from 'node:fs'
import util from 'node:util'

import stripBom from 'strip-bom'

export const YAML_DOCUMENT_SEPARATOR = '\n---\n'
export const YAML_DOCUMENT_START = '---\n'

/**
 * Reads the first YAML document from a multi-document YAML file using streaming.
 * The file must start with "---\n" to indicate it contains an env lockfile document.
 * Stops reading as soon as the second document separator is found.
 * Returns null if the file doesn't exist or doesn't start with "---\n".
 */
export async function streamReadFirstYamlDocument (filePath: string): Promise<string | null> {
  const stream = createReadStream(filePath, { encoding: 'utf8' })
  const chunks = stream[Symbol.asyncIterator]()
  let buffer = ''
  try {
    // Phase 1: verify the file starts with "---\n"

    for (let chunk = await chunks.next(); !chunk.done; chunk = await chunks.next()) { // eslint-disable-line no-await-in-loop
      if (buffer.length === 0) {
        // Strip BOM from the first chunk. Safe because the stream uses utf8 encoding,
        // so the 3-byte BOM is decoded into a single \uFEFF character in the first chunk.
        buffer = stripBom(chunk.value as string)
      } else {
        buffer += chunk.value
      }
      // Normalize CRLF (Windows) to LF so document separator detection works.
      buffer = buffer.replace(/\r\n/g, '\n')
      if (buffer.length >= YAML_DOCUMENT_START.length) break
    }
    if (!buffer.startsWith(YAML_DOCUMENT_START)) {
      await closeStream(stream)
      return null
    }
    // Phase 2: find the second "---" separator
    let firstDocument: string | undefined
    while (true) {
      const sep = buffer.indexOf(YAML_DOCUMENT_SEPARATOR, YAML_DOCUMENT_START.length)
      if (sep !== -1) {
        firstDocument = buffer.slice(YAML_DOCUMENT_START.length, sep)
        break
      }
      const chunk = await chunks.next() // eslint-disable-line no-await-in-loop
      if (chunk.done) break
      // Normalize CRLF (Windows) to LF so the separator search matches on Windows-checked-out files.
      buffer = (buffer + chunk.value).replace(/\r\n/g, '\n')
    }
    if (firstDocument != null) {
      await closeStream(stream)
      return firstDocument
    }
  } catch (err: unknown) {
    await closeStream(stream)
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') {
      return null
    }
    throw err
  }
  await closeStream(stream)
  return null
}

async function closeStream (stream: ReadStream): Promise<void> {
  if (stream.closed) return
  await new Promise<void>((resolve) => {
    stream.once('close', resolve)
    stream.destroy()
  })
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
