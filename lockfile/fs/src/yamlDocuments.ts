import { createReadStream } from 'node:fs'
import util from 'node:util'

import stripBom from 'strip-bom'

const YAML_DOCUMENT_SEPARATOR = '\n---\n'
const YAML_DOCUMENT_START = '---\n'

/**
 * Reads only the first YAML document from a multi-document YAML file using streaming.
 * The file must start with "---\n" to indicate it contains an env lockfile document.
 * Stops reading as soon as the second document separator is found.
 * Returns null if the file doesn't exist or doesn't start with "---\n".
 */
export async function streamReadFirstYamlDocument (filePath: string): Promise<string | null> {
  const stream = createReadStream(filePath, { encoding: 'utf8' })
  let buffer = ''
  try {
    for await (const chunk of stream) {
      buffer += chunk
      // The file must start with "---\n" to contain an env document
      if (buffer.length >= YAML_DOCUMENT_START.length && !buffer.startsWith(YAML_DOCUMENT_START)) {
        stream.destroy()
        return null
      }
      // Find the second "---" separator (the one between env and main documents)
      const sep = buffer.indexOf(YAML_DOCUMENT_SEPARATOR, YAML_DOCUMENT_START.length)
      if (sep !== -1) {
        stream.destroy()
        return stripBom(buffer.slice(YAML_DOCUMENT_START.length, sep))
      }
    }
  } catch (err: unknown) {
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') {
      return null
    }
    throw err
  }
  // No second separator found
  return null
}

/**
 * Reads the env lockfile prefix (first YAML document + separator) from a lockfile using streaming.
 * Returns empty string if the file doesn't exist or doesn't start with "---\n".
 */
export async function readEnvYamlPrefix (lockfilePath: string): Promise<string> {
  const stream = createReadStream(lockfilePath, { encoding: 'utf8' })
  let buffer = ''
  try {
    for await (const chunk of stream) {
      buffer += chunk
      if (buffer.length >= YAML_DOCUMENT_START.length && !buffer.startsWith(YAML_DOCUMENT_START)) {
        stream.destroy()
        return ''
      }
      const sep = buffer.indexOf(YAML_DOCUMENT_SEPARATOR, YAML_DOCUMENT_START.length)
      if (sep !== -1) {
        stream.destroy()
        return buffer.slice(0, sep + YAML_DOCUMENT_SEPARATOR.length)
      }
    }
  } catch (err: unknown) {
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') {
      return ''
    }
    throw err
  }
  return ''
}

/**
 * Extracts the main lockfile content (second YAML document) from a combined string.
 * If the file starts with "---\n", skips past the env document.
 * Otherwise returns the entire content (backwards compatible).
 */
export function extractMainDocument (content: string): string {
  if (!content.startsWith(YAML_DOCUMENT_START)) return content
  const sep = content.indexOf(YAML_DOCUMENT_SEPARATOR, YAML_DOCUMENT_START.length)
  if (sep === -1) return content
  return content.slice(sep + YAML_DOCUMENT_SEPARATOR.length)
}
