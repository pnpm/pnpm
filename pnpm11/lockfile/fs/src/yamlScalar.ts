const INDENT = 2

const CHAR_TAB = 0x09
const CHAR_LINE_FEED = 0x0A
const CHAR_CARRIAGE_RETURN = 0x0D
const CHAR_SPACE = 0x20
const CHAR_SHARP = 0x23
const CHAR_COLON = 0x3A
const CHAR_BOM = 0xFEFF

const FLOW_INDICATORS = new Set([',', '[', ']', '{', '}'].map(codeOf))

const UNSAFE_FIRST_CHARS = new Set(['-', '?', ':', ',', '[', ']', '{', '}', '#', '&', '*', '!', '|', '=', '>', "'", '"', '%', '@', '`'].map(codeOf))

const ESCAPE_SEQUENCES = new Map<number, string>([
  [0x00, '\\0'],
  [0x07, '\\a'],
  [0x08, '\\b'],
  [0x09, '\\t'],
  [0x0A, '\\n'],
  [0x0B, '\\v'],
  [0x0C, '\\f'],
  [0x0D, '\\r'],
  [0x1B, '\\e'],
  [0x22, '\\"'],
  [0x5C, '\\\\'],
  [0x85, '\\N'],
  [0xA0, '\\_'],
  [0x2028, '\\L'],
  [0x2029, '\\P'],
])

export interface ScalarContext {
  level: number
  /** Keys and flow-style scalars have no room for a line break. */
  singleLineOnly: boolean
  /** Whether the scalar sits in a block collection, where flow indicators are plain-safe. */
  inBlock: boolean
  /** Whether a plain scalar would read back as something other than this string. */
  isAmbiguous: (value: string) => boolean
}

type ScalarStyle = 'plain' | 'single' | 'double' | 'literal'

/** Writes a string scalar the way js-yaml 4's `writeScalar` does with single quotes, `lineWidth: -1`, and `noCompatMode`. */
export function writeScalar (value: string, ctx: ScalarContext): string {
  if (value === '') return "''"
  switch (chooseScalarStyle(value, ctx)) {
    case 'plain': return value
    case 'single': return `'${value.replaceAll("'", "''")}'`
    case 'double': return `"${escapeString(value)}"`
    case 'literal': return `|${blockHeader(value)}${dropEndingNewline(indentString(value, INDENT * Math.max(1, ctx.level)))}`
  }
}

function chooseScalarStyle (value: string, ctx: ScalarContext): ScalarStyle {
  let plain = isPlainSafeFirst(value.codePointAt(0)!) && isPlainSafeLast(value.charCodeAt(value.length - 1))
  let hasLineBreak = false
  let prevChar: number | undefined
  for (const char of value) {
    const code = char.codePointAt(0)!
    if (code === CHAR_LINE_FEED && !ctx.singleLineOnly) {
      hasLineBreak = true
    } else if (!isPrintable(code)) {
      return 'double'
    }
    plain = plain && isPlainSafe(code, prevChar, ctx.inBlock)
    prevChar = code
  }
  if (hasLineBreak) return 'literal'
  return plain && !ctx.isAmbiguous(value) ? 'plain' : 'single'
}

function isPrintable (code: number): boolean {
  return (code >= 0x20 && code <= 0x7E) ||
    (code >= 0xA1 && code <= 0xD7FF && code !== 0x2028 && code !== 0x2029) ||
    (code >= 0xE000 && code <= 0xFFFD && code !== CHAR_BOM) ||
    (code >= 0x10000 && code <= 0x10FFFF)
}

function isWhitespace (code: number | undefined): boolean {
  return code === CHAR_SPACE || code === CHAR_TAB
}

function isNsCharOrWhitespace (code: number | undefined): boolean {
  return code != null && isPrintable(code) && code !== CHAR_BOM && code !== CHAR_CARRIAGE_RETURN && code !== CHAR_LINE_FEED
}

function isNsChar (code: number | undefined): boolean {
  return isNsCharOrWhitespace(code) && !isWhitespace(code)
}

function isPlainSafeFirst (code: number): boolean {
  return isPrintable(code) && code !== CHAR_BOM && !isWhitespace(code) && !UNSAFE_FIRST_CHARS.has(code)
}

function isPlainSafeLast (code: number): boolean {
  return !isWhitespace(code) && code !== CHAR_COLON
}

/** Looser than YAML's `ns-plain-char`, as in js-yaml: a space is allowed, and so is a flow indicator after a `:`. */
function isPlainSafe (code: number, prev: number | undefined, inBlock: boolean): boolean {
  if (code === CHAR_SHARP) return isNsChar(prev)
  if (prev === CHAR_COLON) return isNsChar(code)
  return isNsCharOrWhitespace(code) && (inBlock || !FLOW_INDICATORS.has(code))
}

function escapeString (value: string): string {
  let result = ''
  for (const char of value) {
    const code = char.codePointAt(0)!
    const escapeSequence = ESCAPE_SEQUENCES.get(code)
    if (escapeSequence != null) {
      result += escapeSequence
    } else if (isPrintable(code)) {
      result += char
    } else {
      result += encodeHex(code)
    }
  }
  return result
}

function encodeHex (code: number): string {
  const hex = code.toString(16).toUpperCase()
  if (code <= 0xFF) return `\\x${hex.padStart(2, '0')}`
  if (code <= 0xFFFF) return `\\u${hex.padStart(4, '0')}`
  return `\\U${hex.padStart(8, '0')}`
}

function blockHeader (value: string): string {
  const indentIndicator = value.replace(/^\n*/, '').startsWith(' ') ? String(INDENT) : ''
  const clip = value.endsWith('\n')
  const keep = clip && (value.endsWith('\n\n') || value === '\n')
  const chomp = keep ? '+' : clip ? '' : '-'
  return `${indentIndicator}${chomp}\n`
}

function dropEndingNewline (value: string): string {
  return value.endsWith('\n') ? value.slice(0, -1) : value
}

function indentString (value: string, spaces: number): string {
  const indent = ' '.repeat(spaces)
  return value.split(/(?<=\n)/).map((line) => line !== '' && line !== '\n' ? indent + line : line).join('')
}

function codeOf (char: string): number {
  return char.charCodeAt(0)
}
