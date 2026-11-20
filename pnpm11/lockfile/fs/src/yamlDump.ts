import { resolvesImplicitly } from './yamlImplicitTypes.js'
import { writeScalar } from './yamlScalar.js'

export interface YamlDumpOptions {
  /** Nesting level from which collections are written in flow style. */
  flowLevel?: number
}

/**
 * Serializes a JSON-like value to YAML exactly as js-yaml 4's `dump` does with
 * `{ lineWidth: -1, noCompatMode: true, noRefs: true }`. Lockfile formatting
 * depends on that output, so it is pinned here instead of following js-yaml.
 */
export function dumpYaml (value: unknown, options?: YamlDumpOptions): string {
  const state: DumpState = { flowLevel: options?.flowLevel ?? -1 }
  const rendered = writeNode(state, value, { level: 0, block: true, compact: true, isKey: false })
  return rendered == null ? '' : `${rendered}\n`
}

const INDENT = 2
const EXPLICIT_KEY_THRESHOLD = 1024

interface DumpState {
  flowLevel: number
}

interface NodeContext {
  level: number
  block: boolean
  compact: boolean
  isKey: boolean
}

function writeNode (state: DumpState, value: unknown, ctx: NodeContext): string | null {
  if (value === undefined) return null
  if (Array.isArray(value) || isPlainObject(value)) return writeCollection(state, value, ctx)
  return writeScalarNode(state, value, ctx)
}

function writeScalarNode (state: DumpState, value: unknown, ctx: NodeContext): string {
  if (value === null) return 'null'
  if (typeof value === 'boolean') return String(value)
  if (typeof value === 'number') return representNumber(value)
  if (typeof value !== 'string') {
    throw new Error(`Cannot serialize a value of type ${Object.prototype.toString.call(value)} to YAML`)
  }
  return writeScalar(value, {
    level: ctx.level,
    singleLineOnly: ctx.isKey || (state.flowLevel > -1 && ctx.level >= state.flowLevel),
    inBlock: ctx.block,
    isAmbiguous: resolvesImplicitly,
  })
}

function writeCollection (state: DumpState, value: unknown[] | Record<string, unknown>, ctx: NodeContext): string {
  const block = ctx.block && (state.flowLevel < 0 || state.flowLevel > ctx.level)
  if (Array.isArray(value)) {
    return block && value.length > 0
      ? writeBlockSequence(state, value, ctx.level, ctx.compact)
      : writeFlowSequence(state, value, ctx.level)
  }
  return block && Object.keys(value).length > 0
    ? writeBlockMapping(state, value, ctx.level, ctx.compact)
    : writeFlowMapping(state, value, ctx.level)
}

function writeBlockSequence (state: DumpState, sequence: unknown[], level: number, compact: boolean): string {
  let result = ''
  for (const item of sequence) {
    const rendered = writeSequenceItem(state, item, { level: level + 1, block: true, compact: true, isKey: false })
    if (!compact || result !== '') result += nextLine(level)
    result += rendered.startsWith('\n') ? '-' : '- '
    result += rendered
  }
  return result || '[]'
}

function writeFlowSequence (state: DumpState, sequence: unknown[], level: number): string {
  const items = Array.from(sequence, (item) => writeSequenceItem(state, item, { level, block: false, compact: false, isKey: false }))
  return `[${items.join(', ')}]`
}

/** An `undefined` sequence item is written as `null`, as js-yaml does. */
function writeSequenceItem (state: DumpState, item: unknown, ctx: NodeContext): string {
  return writeNode(state, item, ctx) ?? writeNode(state, null, ctx)!
}

function writeBlockMapping (state: DumpState, mapping: Record<string, unknown>, level: number, compact: boolean): string {
  let result = ''
  for (const [key, value] of Object.entries(mapping)) {
    const pair = writeBlockPair(state, [key, value], level)
    if (pair == null) continue
    if (!compact || result !== '') result += nextLine(level)
    result += pair
  }
  return result || '{}'
}

/** A key too long for a simple key is written as an explicit `? key` / `: value` pair. */
function writeBlockPair (state: DumpState, [key, value]: [string, unknown], level: number): string | null {
  const renderedKey = writeNode(state, key, { level: level + 1, block: true, compact: true, isKey: true })!
  const explicitPair = renderedKey.length > EXPLICIT_KEY_THRESHOLD
  const renderedValue = writeNode(state, value, { level: level + 1, block: true, compact: explicitPair, isKey: false })
  if (renderedValue == null) return null
  const keyPart = explicitPair ? `? ${renderedKey}${nextLine(level)}` : renderedKey
  return `${keyPart}${renderedValue.startsWith('\n') ? ':' : ': '}${renderedValue}`
}

function writeFlowMapping (state: DumpState, mapping: Record<string, unknown>, level: number): string {
  const pairs: string[] = []
  for (const [key, value] of Object.entries(mapping)) {
    const ctx: NodeContext = { level, block: false, compact: false, isKey: false }
    const renderedKey = writeNode(state, key, ctx)!
    const renderedValue = writeNode(state, value, ctx)
    if (renderedValue == null) continue
    const explicitMarker = renderedKey.length > EXPLICIT_KEY_THRESHOLD ? '? ' : ''
    pairs.push(`${explicitMarker}${renderedKey}: ${renderedValue}`)
  }
  return `{${pairs.join(', ')}}`
}

function nextLine (level: number): string {
  return `\n${' '.repeat(INDENT * level)}`
}

function representNumber (value: number): string {
  if (Number.isNaN(value)) return '.nan'
  if (value === Infinity) return '.inf'
  if (value === -Infinity) return '-.inf'
  if (Object.is(value, -0)) return '-0.0'
  const rendered = value.toString(10)
  // js-yaml adds a dot to an exponent-only float so that it reads back as a float.
  return Number.isInteger(value) || rendered.includes('.') || !rendered.includes('e') ? rendered : rendered.replace('e', '.e')
}

function isPlainObject (value: unknown): value is Record<string, unknown> {
  return Object.prototype.toString.call(value) === '[object Object]'
}
