import { dumpYaml } from './yamlDump.js'

const FLOW_OPTIONS = { flowLevel: 1 }
const INLINE_KEYS = new Set(['cpu', 'engines', 'os', 'libc'])
const SPACED_SECTIONS = new Set(['importers', 'packages', 'snapshots'])
const INDENT = '  '

export function lockfileYamlDump (lockfile: object): string {
  return `${writeMapping(lockfile as Record<string, unknown>, '\n\n')}\n`
}

function writeMapping (mapping: Record<string, unknown>, separator: string): string {
  const chunks = writeMappingChunks(mapping, separator === '\n')
  if (chunks.length === 0) return '{}'
  return separator === '\n' ? chunks.join(separator) : joinWithBlankLines(chunks)
}

function writeMappingChunks (mapping: Record<string, unknown>, batchOrdinaryEntries: boolean): string[] {
  const entries = Object.entries(mapping)
    .filter(([, value]) => value !== undefined)
  const chunks: string[] = []
  let ordinaryEntries: Array<[string, unknown]> = []
  for (const [key, value] of entries) {
    const custom = hasCustomLayout(key, value)
    if (!custom && batchOrdinaryEntries) {
      ordinaryEntries.push([key, value])
      continue
    }
    if (ordinaryEntries.length > 0) {
      chunks.push(dumpYaml(Object.fromEntries(ordinaryEntries)).slice(0, -1))
      ordinaryEntries = []
    }
    chunks.push(custom ? writeCustomEntry(key, value) : dumpYaml({ [key]: value }).slice(0, -1))
  }
  if (ordinaryEntries.length > 0) {
    chunks.push(dumpYaml(Object.fromEntries(ordinaryEntries)).slice(0, -1))
  }
  return chunks
}

function joinWithBlankLines (chunks: string[]): string {
  let result = chunks[0]
  for (let index = 1; index < chunks.length; index++) {
    // A trailing newline belongs to a block scalar that keeps its blank lines.
    const separator = chunks[index - 1].endsWith('\n') ? '\n' : '\n\n'
    result += separator + chunks[index]
  }
  return result
}

function writeCustomEntry (key: string, value: Record<string, unknown> | unknown[]): string {
  if (usesFlowStyle(key, value)) {
    return dumpYaml({ [key]: value }, FLOW_OPTIONS).slice(0, -1)
  }

  const renderedKey = dumpYaml({ [key]: null }).slice(0, -' null\n'.length)
  return writeBlockEntry(key, renderedKey, value)
}

function hasCustomLayout (key: string, value: unknown): value is Record<string, unknown> | unknown[] {
  if (Array.isArray(value)) return value.length > 0 && (INLINE_KEYS.has(key) || value.some(hasNestedLayout))
  if (!isMapping(value)) return false
  const keys = Object.keys(value)
  return keys.length > 0 && (
    SPACED_SECTIONS.has(key) || usesFlowStyle(key, value) || keys.some((key) => hasCustomLayout(key, value[key]))
  )
}

function hasNestedLayout (value: unknown): value is Record<string, unknown> | unknown[] {
  if (Array.isArray(value)) return value.some(hasNestedLayout)
  return isMapping(value) && Object.keys(value).some((key) => hasCustomLayout(key, value[key]))
}

function usesFlowStyle (key: string, value: Record<string, unknown> | unknown[]): boolean {
  return INLINE_KEYS.has(key) ||
    (key === 'resolution' && !Array.isArray(value) && value.type !== 'variations' && value.type !== 'binary')
}

function writeBlockEntry (key: string, renderedKey: string, value: Record<string, unknown> | unknown[]): string {
  const mapping = !Array.isArray(value)
  const separator = mapping && SPACED_SECTIONS.has(key) ? '\n\n' : '\n'
  const collection = mapping ? writeMapping(value, separator) : writeSequence(value)
  // For explicit keys, the first collection entry follows the colon on the same line.
  if (collection === '{}' || renderedKey.startsWith('? ')) {
    return `${renderedKey} ${indentContinuation(collection)}`
  }
  return `${renderedKey}${separator}${INDENT}${indentContinuation(collection)}`
}

function writeSequence (sequence: unknown[]): string {
  return Array.from(sequence, writeSequenceItem).join('\n')
}

function writeSequenceItem (item: unknown): string {
  if (!hasNestedLayout(item)) return dumpYaml([item]).slice(0, -1)
  const collection = Array.isArray(item) ? writeSequence(item) : writeMapping(item, '\n')
  return `- ${indentContinuation(collection)}`
}

function isMapping (value: unknown): value is Record<string, unknown> {
  return Object.prototype.toString.call(value) === '[object Object]'
}

function indentContinuation (value: string): string {
  // Preserve empty lines, including trailing lines kept by block scalars.
  return value.replace(/\n(?=.)/g, `\n${INDENT}`)
}
