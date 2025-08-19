import assert from 'assert/strict'
import { PnpmError } from '@pnpm/error'
import {
  type ExactToken,
  type Identifier,
  type NumericLiteral,
  type StringLiteral,
  type UnexpectedToken,
  tokenize,
} from './token/index.js'

export class UnexpectedTokenError<Token extends ExactToken<string> | UnexpectedToken> extends PnpmError {
  readonly token: Token
  constructor (token: Token) {
    super('UNEXPECTED_TOKEN_IN_PROPERTY_PATH', `Unexpected token ${JSON.stringify(token.content)} in property path`)
    this.token = token
  }
}

export class UnexpectedIdentifierError extends PnpmError {
  readonly token: Identifier
  constructor (token: Identifier) {
    super('UNEXPECTED_IDENTIFIER_IN_PROPERTY_PATH', `Unexpected identifier ${token.content} in property path`)
    this.token = token
  }
}

export class UnexpectedLiteralError extends PnpmError {
  readonly token: NumericLiteral | StringLiteral
  constructor (token: NumericLiteral | StringLiteral) {
    super('UNEXPECTED_LITERAL_IN_PROPERTY_PATH', `Unexpected literal ${JSON.stringify(token.content)} in property path`)
    this.token = token
  }
}

export class UnexpectedEndOfInputError extends PnpmError {
  constructor () {
    super('UNEXPECTED_END_OF_PROPERTY_PATH', 'The property path does not end properly')
  }
}

/**
 * Parse a string of property path.
 *
 * @example
 *   parsePropertyPath('foo.bar.baz')
 *   parsePropertyPath('.foo.bar.baz')
 *   parsePropertyPath('foo.bar["baz"]')
 *   parsePropertyPath("foo['bar'].baz")
 *   parsePropertyPath('["foo"].bar.baz')
 *   parsePropertyPath(`["foo"]['bar'].baz`)
 *   parsePropertyPath('foo[123]')
 *
 * @param propertyPath The string of property path to parse.
 * @returns The parsed path in the form of an array.
 */
export function * parsePropertyPath (propertyPath: string): Generator<string | number, void, void> {
  type Stack =
    | ExactToken<'.'>
    | ExactToken<'['>
    | [ExactToken<'['>, NumericLiteral | StringLiteral]
  let stack: Stack | undefined

  for (const token of tokenize(propertyPath)) {
    if (token.type === 'exact' && token.content === '.') {
      if (!stack) {
        stack = token
        continue
      }

      throw new UnexpectedTokenError(token)
    }

    if (token.type === 'exact' && token.content === '[') {
      if (!stack) {
        stack = token
        continue
      }

      throw new UnexpectedTokenError(token)
    }

    if (token.type === 'exact' && token.content === ']') {
      if (!Array.isArray(stack)) throw new UnexpectedTokenError(token)

      const [openBracket, literal] = stack
      assert.equal(openBracket.type, 'exact')
      assert.equal(openBracket.content, '[')
      assert(literal.type === 'numeric-literal' || literal.type === 'string-literal')

      yield literal.content
      stack = undefined
      continue
    }

    if (token.type === 'identifier') {
      if (!stack || ('type' in stack && stack.type === 'exact' && stack.content === '.')) {
        stack = undefined
        yield token.content
        continue
      }

      throw new UnexpectedIdentifierError(token)
    }

    if (token.type === 'numeric-literal' || token.type === 'string-literal') {
      if (stack && 'type' in stack && stack.type === 'exact' && stack.content === '[') {
        stack = [stack, token]
        continue
      }

      throw new UnexpectedLiteralError(token)
    }

    if (token.type === 'whitespace') continue
    if (token.type === 'unexpected') throw new UnexpectedTokenError(token)

    const _typeGuard: never = token // eslint-disable-line @typescript-eslint/no-unused-vars
  }

  if (stack) throw new UnexpectedEndOfInputError()
}
