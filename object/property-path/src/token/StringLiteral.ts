import { ParseErrorBase } from './ParseErrorBase.js'
import { type TokenBase, type Tokenize } from './types.js'

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
  let quote: StringLiteralQuote
  if (source[0] === '"') {
    quote = '"'
  } else if (source[0] === "'") {
    quote = "'"
  } else {
    return undefined
  }

  source = source.slice(1)
  let content = ''
  let escaped = false

  while (source !== '') {
    const char = source[0]
    source = source.slice(1)

    if (escaped) {
      escaped = false
      const realChar = STRING_LITERAL_ESCAPES[char]
      if (!realChar) {
        throw new UnsupportedEscapeSequenceError(char)
      }
      content += realChar
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
