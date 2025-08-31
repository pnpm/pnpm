import { type ExactToken, parseCloseBracket, parseDotOperator, parseOpenBracket } from './ExactToken.js'
import { type Identifier, parseIdentifier } from './Identifier.js'
import { type NumericLiteral, parseNumericLiteral } from './NumericLiteral.js'
import { type StringLiteral, parseStringLiteral } from './StringLiteral.js'
import { type Whitespace, parseWhitespace } from './Whitespace.js'
import { combineParsers } from './combine.js'
import { type TokenBase, type Tokenize } from './types.js'

export type ExpectedToken =
  | ExactToken<'.'>
  | ExactToken<'['>
  | ExactToken<']'>
  | Identifier
  | NumericLiteral
  | StringLiteral
  | Whitespace

export const parseExpectedToken: Tokenize<ExpectedToken> = combineParsers<ExpectedToken>([
  parseDotOperator,
  parseOpenBracket,
  parseCloseBracket,
  parseIdentifier,
  parseNumericLiteral,
  parseStringLiteral,
  parseWhitespace,
])

export interface UnexpectedToken extends TokenBase {
  type: 'unexpected'
  content: string
}

const parseUnexpectedToken: Tokenize<UnexpectedToken> = source =>
  [{ type: 'unexpected', content: source.slice(0, 1) }, source.slice(1)]

export type Token = ExpectedToken | UnexpectedToken
export const parseToken = combineParsers<Token>([parseExpectedToken, parseUnexpectedToken])

/** Generate all tokens from a source text. */
export function * tokenize (source: string): Generator<Token, void, void> {
  while (source !== '') {
    const parseResult = parseToken(source)
    if (!parseResult) break

    const [token, remaining] = parseResult
    yield token

    // guard against programmer error
    if (source.length <= remaining.length) {
      throw new Error(`Something went wrong! the remaining string (${remaining}) is supposed to be less than the source string (${source})`)
    }

    source = remaining
  }
}
