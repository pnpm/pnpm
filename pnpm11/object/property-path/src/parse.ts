import assert from 'node:assert/strict'

import { PnpmError } from '@pnpm/error'

import {
  type ExactToken,
  type Identifier,
  type NumericLiteral,
  type StringLiteral,
  type Token,
  tokenize,
  type UnexpectedToken,
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
 */
type ParseStack =
  | ExactToken<'.'>
  | ExactToken<'['>
  | [ExactToken<'['>, NumericLiteral | StringLiteral]

export function * parsePropertyPath (propertyPath: string): Generator<string | number, void, void> {
  let stack: ParseStack | undefined

  for (const token of tokenize(propertyPath)) {
    const step = processPropertyToken(token, stack)
    stack = step.nextStack
    if (step.emittedSegment != null) {
      yield step.emittedSegment
    }
  }

  if (stack) throw new UnexpectedEndOfInputError()
}

interface StepResult {
  nextStack: ParseStack | undefined
  emittedSegment?: string | number
}

function processPropertyToken (token: Token, stack: ParseStack | undefined): StepResult {
  if (token.type === 'exact') {
    return handleExactToken(token, stack)
  }
  if (token.type === 'identifier') {
    return handleIdentifierToken(token, stack)
  }
  if (token.type === 'numeric-literal' || token.type === 'string-literal') {
    return handleLiteralToken(token, stack)
  }
  if (token.type === 'whitespace') {
    return { nextStack: stack }
  }
  if (token.type === 'unexpected') {
    throw new UnexpectedTokenError(token)
  }
  const _typeGuard: never = token
  return { nextStack: stack }
}

function handleExactToken (token: ExactToken<string>, stack: ParseStack | undefined): StepResult {
  if (token.content === '.' || token.content === '[') {
    if (!stack) return { nextStack: token as ExactToken<'.' | '['> }
    throw new UnexpectedTokenError(token)
  }
  if (token.content === ']') {
    if (!Array.isArray(stack)) throw new UnexpectedTokenError(token)
    const [openBracket, literal] = stack
    assert.equal(openBracket.type, 'exact')
    assert.equal(openBracket.content, '[')
    assert(literal.type === 'numeric-literal' || literal.type === 'string-literal')
    return { emittedSegment: literal.content, nextStack: undefined }
  }
  throw new UnexpectedTokenError(token)
}

function handleIdentifierToken (token: Identifier, stack: ParseStack | undefined): StepResult {
  if (!stack || ('type' in stack && stack.type === 'exact' && stack.content === '.')) {
    return { emittedSegment: token.content, nextStack: undefined }
  }
  throw new UnexpectedIdentifierError(token)
}

function handleLiteralToken (token: NumericLiteral | StringLiteral, stack: ParseStack | undefined): StepResult {
  if (stack && 'type' in stack && stack.type === 'exact' && stack.content === '[') {
    return { nextStack: [stack, token] }
  }
  throw new UnexpectedLiteralError(token)
}
