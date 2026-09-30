import { ParseErrorBase } from './ParseErrorBase.js'
import type { TokenBase, Tokenize } from './types.js'

export type StringLiteralQuote = '"' | "'"

export interface StringLiteral extends TokenBase {
  type: 'string-literal'
  quote: StringLiteralQuote
  content: string
}

const STRING_LITERAL_ESCAPES: Record<string, string | undefined> = {
  '\\': '\\',
  "'": "'",
  '"': '"',
  b: '\b',
  n: '\n',
  r: '\r',
  t: '\t',
}

export class UnsupportedEscapeSequenceError extends ParseErrorBase {
  readonly sequence: string
  constructor (sequence: string) {
    super('UNSUPPORTED_STRING_LITERAL_ESCAPE_SEQUENCE', `pnpm's string literal doesn't support ${JSON.stringify('\\' + sequence)}`)
    this.sequence = sequence
  }
}

export class IncompleteStringLiteralError extends ParseErrorBase {
  readonly expectedQuote: StringLiteralQuote
  constructor (expectedQuote: StringLiteralQuote) {
    super('INCOMPLETE_STRING_LITERAL', `Input ends without closing quote (${expectedQuote})`)
    this.expectedQuote = expectedQuote
  }
}

export const parseStringLiteral: Tokenize<StringLiteral> = source => {
  const quote = detectQuote(source[0])
  if (quote == null) return undefined

  source = source.slice(1)
  let content = ''
  let escaped = false

  while (source !== '') {
    const char = source[0]
    source = source.slice(1)

    if (escaped) {
      escaped = false
      content += resolveEscapedChar(char)
      continue
    }

    if (char === quote) {
      return [{ type: 'string-literal', quote, content }, source]
    }

    if (char === '\\') {
      escaped = true
      continue
    }

    content += char
  }

  throw new IncompleteStringLiteralError(quote)
}

function detectQuote (firstChar: string | undefined): StringLiteralQuote | undefined {
  if (firstChar === '"' || firstChar === "'") return firstChar
  return undefined
}

function resolveEscapedChar (char: string): string {
  const realChar = STRING_LITERAL_ESCAPES[char]
  if (!realChar) {
    throw new UnsupportedEscapeSequenceError(char)
  }
  return realChar
}
