import { ParseErrorBase } from './ParseErrorBase.js'
import { type TokenBase, type Tokenize } from './types.js'

export interface NumericLiteral extends TokenBase {
  type: 'numeric-literal'
  content: number
}

export class UnsupportedNumericSuffix extends ParseErrorBase {
  readonly suffix: string
  constructor (suffix: string) {
    super('UNSUPPORTED_NUMERIC_LITERAL_SUFFIX', `Numeric suffix ${JSON.stringify(suffix)} is not supported`)
    this.suffix = suffix
  }
}

export const parseNumericLiteral: Tokenize<NumericLiteral> = source => {
  if (source === '') return undefined

  const firstChar = source[0]
  if (firstChar < '0' || firstChar > '9') return undefined

  let numberString = firstChar
  source = source.slice(1)

  while (source !== '') {
    const char = source[0]

    if (/[0-9.]/.test(char)) {
      numberString += char
      source = source.slice(1)
      continue
    }

    // We forbid things like `0x1A2E`, `1e20`, or `123n` for now.
    if (/[a-z]/i.test(char)) {
      throw new UnsupportedNumericSuffix(char)
    }

    break
  }

  return [{ type: 'numeric-literal', content: Number(numberString) }, source]
}
