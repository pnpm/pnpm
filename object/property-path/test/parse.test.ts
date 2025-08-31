import {
  type ExactToken,
  type UnexpectedEndOfInputError,
  type UnexpectedIdentifierError,
  type UnexpectedLiteralError,
  type UnexpectedToken,
  type UnexpectedTokenError,
  parsePropertyPath,
} from '../src/index.js'

test('valid property path', () => {
  expect(Array.from(parsePropertyPath(''))).toStrictEqual([])
  expect(Array.from(parsePropertyPath('foo'))).toStrictEqual(['foo'])
  expect(Array.from(parsePropertyPath('.foo'))).toStrictEqual(['foo'])
  expect(Array.from(parsePropertyPath('["foo"]'))).toStrictEqual(['foo'])
  expect(Array.from(parsePropertyPath("['foo']"))).toStrictEqual(['foo'])
  expect(Array.from(parsePropertyPath('[ "foo" ]'))).toStrictEqual(['foo'])
  expect(Array.from(parsePropertyPath("[ 'foo' ]"))).toStrictEqual(['foo'])
  expect(Array.from(parsePropertyPath('foo.bar[0]'))).toStrictEqual(['foo', 'bar', 0])
  expect(Array.from(parsePropertyPath('.foo.bar[0]'))).toStrictEqual(['foo', 'bar', 0])
  expect(Array.from(parsePropertyPath('foo["bar"][0]'))).toStrictEqual(['foo', 'bar', 0])
  expect(Array.from(parsePropertyPath(".foo['bar'][0]"))).toStrictEqual(['foo', 'bar', 0])
  expect(Array.from(parsePropertyPath('foo.bar["0"]'))).toStrictEqual(['foo', 'bar', '0'])
  expect(Array.from(parsePropertyPath('a.b.c.d'))).toStrictEqual(['a', 'b', 'c', 'd'])
  expect(Array.from(parsePropertyPath('.a.b.c.d'))).toStrictEqual(['a', 'b', 'c', 'd'])
  expect(Array.from(parsePropertyPath('a .b .c .d'))).toStrictEqual(['a', 'b', 'c', 'd'])
  expect(Array.from(parsePropertyPath('.a .b .c .d'))).toStrictEqual(['a', 'b', 'c', 'd'])
})

test('invalid property path', () => {
  expect(() => Array.from(parsePropertyPath('foo.bar.0'))).toThrow(expect.objectContaining({
    code: 'ERR_PNPM_UNEXPECTED_LITERAL_IN_PROPERTY_PATH',
    token: {
      type: 'numeric-literal',
      content: 0,
    },
  } as Partial<UnexpectedLiteralError>))
  expect(() => Array.from(parsePropertyPath('foo.bar."baz"'))).toThrow(expect.objectContaining({
    code: 'ERR_PNPM_UNEXPECTED_LITERAL_IN_PROPERTY_PATH',
    token: {
      type: 'string-literal',
      quote: '"',
      content: 'baz',
    },
  } as Partial<UnexpectedLiteralError>))
  expect(() => Array.from(parsePropertyPath('foo.bar"baz"'))).toThrow(expect.objectContaining({
    code: 'ERR_PNPM_UNEXPECTED_LITERAL_IN_PROPERTY_PATH',
    token: {
      type: 'string-literal',
      quote: '"',
      content: 'baz',
    },
  } as Partial<UnexpectedLiteralError>))
  expect(() => Array.from(parsePropertyPath('foo.bar "baz"'))).toThrow(expect.objectContaining({
    code: 'ERR_PNPM_UNEXPECTED_LITERAL_IN_PROPERTY_PATH',
    token: {
      type: 'string-literal',
      quote: '"',
      content: 'baz',
    },
  } as Partial<UnexpectedLiteralError>))
  expect(() => Array.from(parsePropertyPath('foo.bar[baz]'))).toThrow(expect.objectContaining({
    code: 'ERR_PNPM_UNEXPECTED_IDENTIFIER_IN_PROPERTY_PATH',
    token: {
      type: 'identifier',
      content: 'baz',
    },
  } as Partial<UnexpectedIdentifierError>))
  expect(() => Array.from(parsePropertyPath('foo.bar..baz'))).toThrow(expect.objectContaining({
    code: 'ERR_PNPM_UNEXPECTED_TOKEN_IN_PROPERTY_PATH',
    token: {
      type: 'exact',
      content: '.',
    },
  } as Partial<UnexpectedTokenError<ExactToken<'.'>>>))
  expect(() => Array.from(parsePropertyPath('foo.bar[[0]]'))).toThrow(expect.objectContaining({
    code: 'ERR_PNPM_UNEXPECTED_TOKEN_IN_PROPERTY_PATH',
    token: {
      type: 'exact',
      content: '[',
    },
  } as Partial<UnexpectedTokenError<ExactToken<'['>>>))
  expect(() => Array.from(parsePropertyPath('foo.bar[0]]'))).toThrow(expect.objectContaining({
    code: 'ERR_PNPM_UNEXPECTED_TOKEN_IN_PROPERTY_PATH',
    token: {
      type: 'exact',
      content: ']',
    },
  } as Partial<UnexpectedTokenError<ExactToken<']'>>>))
  expect(() => Array.from(parsePropertyPath('foo.bar?.baz'))).toThrow(expect.objectContaining({
    code: 'ERR_PNPM_UNEXPECTED_TOKEN_IN_PROPERTY_PATH',
    token: {
      type: 'unexpected',
      content: '?',
    },
  } as Partial<UnexpectedTokenError<UnexpectedToken>>))
  expect(() => Array.from(parsePropertyPath('foo.bar.baz.'))).toThrow(expect.objectContaining({
    code: 'ERR_PNPM_UNEXPECTED_END_OF_PROPERTY_PATH',
  } as Partial<UnexpectedEndOfInputError>))
  expect(() => Array.from(parsePropertyPath('foo.bar.baz[0'))).toThrow(expect.objectContaining({
    code: 'ERR_PNPM_UNEXPECTED_END_OF_PROPERTY_PATH',
  } as Partial<UnexpectedEndOfInputError>))
})

test('partial parse', () => {
  const iter = parsePropertyPath('.foo.bar[123]?.baz')
  expect(iter.next()).toStrictEqual({ done: false, value: 'foo' })
  expect(iter.next()).toStrictEqual({ done: false, value: 'bar' })
  expect(iter.next()).toStrictEqual({ done: false, value: 123 })
  expect(() => iter.next()).toThrow(expect.objectContaining({
    code: 'ERR_PNPM_UNEXPECTED_TOKEN_IN_PROPERTY_PATH',
    token: {
      type: 'unexpected',
      content: '?',
    },
  } as Partial<UnexpectedTokenError<UnexpectedToken>>))
  expect(iter.next()).toStrictEqual({ done: true, value: undefined })
})
