import { parseYAML } from 'confbox/yaml'

/**
 * The value of the YAML document in `content`, or `null` when it holds none:
 * only blank lines, comments, and `...` document end markers, which pnpm
 * reads as an empty file. `parseYAML` rejects such content.
 */
export function parseYamlDocument (content: string): unknown {
  return holdsYamlDocument(content) ? parseYAML(content) : null
}

function holdsYamlDocument (content: string): boolean {
  return content.split(/\r\n?|\n/).some((line) => {
    const trimmed = line.trim()
    return trimmed !== '' && !trimmed.startsWith('#') && !DOCUMENT_END_MARKER.test(line)
  })
}

/** `...` at the start of a line, alone or before a comment. */
const DOCUMENT_END_MARKER = /^\ufeff?\.\.\.(?:\s+#.*|\s*)$/
