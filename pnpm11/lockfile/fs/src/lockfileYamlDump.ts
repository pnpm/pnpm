import { dump } from 'js-yaml'

const OPTIONS = { lineWidth: -1, noCompatMode: true, noRefs: true }
const FLOW_OPTIONS = { ...OPTIONS, flowLevel: 0 }
const INLINE_KEYS = new Set(['cpu', 'engines', 'os', 'libc'])
const SPACED_SECTIONS = new Set(['importers', 'packages', 'snapshots'])
const INDENT = '  '

export function lockfileYamlDump (lockfile: object): string {
  return `${writeMapping(lockfile as Record<string, unknown>, '\n\n')}\n`
}

function writeMapping (mapping: Record<string, unknown>, separator: string): string {
  const entries = Object.entries(mapping)
    .filter(([, value]) => value !== undefined)
    .map(([key, value]) => writeEntry(key, value))
  return entries.length === 0 ? '{}' : entries.join(separator)
}

function writeEntry (key: string, value: unknown): string {
  if (!hasCustomLayout(key, value)) {
    return dump({ [key]: value }, OPTIONS).slice(0, -1)
  }

  const renderedKey = dump({ [key]: null }, OPTIONS).slice(0, -' null\n'.length)
  if (usesFlowStyle(key, value)) {
    return `${renderedKey} ${dump(value, FLOW_OPTIONS).slice(0, -1)}`
  }

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
  if (!hasNestedLayout(item)) return dump([item], OPTIONS).slice(0, -1)
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
