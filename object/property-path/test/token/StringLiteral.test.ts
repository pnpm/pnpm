import { type StringLiteral, parseStringLiteral } from '../../src/index.js'

test('not a string literal', () => {
  expect(parseStringLiteral('')).toBeUndefined()
  expect(parseStringLiteral('not a string')).toBeUndefined()
  expect(parseStringLiteral('not a string again "this string would be ignored"')).toBeUndefined()
  expect(parseStringLiteral('0123')).toBeUndefined()
})

test('simple string literal', () => {
  expect(parseStringLiteral('""')).toStrictEqual([{
    type: 'string-literal',
    quote: '"',
    content: '',
  } as StringLiteral, ''])
  expect(parseStringLiteral("''")).toStrictEqual([{
    type: 'string-literal',
    quote: "'",
    content: '',
  } as StringLiteral, ''])
  expect(parseStringLiteral('"hello world"')).toStrictEqual([{
    type: 'string-literal',
    quote: '"',
    content: 'hello world',
  } as StringLiteral, ''])
  expect(parseStringLiteral("'hello world'")).toStrictEqual([{
    type: 'string-literal',
    quote: "'",
    content: 'hello world',
  } as StringLiteral, ''])
  expect(parseStringLiteral('"hello world".length')).toStrictEqual([{
    type: 'string-literal',
    quote: '"',
    content: 'hello world',
  } as StringLiteral, '.length'])
  expect(parseStringLiteral("'hello world'.length")).toStrictEqual([{
    type: 'string-literal',
    quote: "'",
    content: 'hello world',
  } as StringLiteral, '.length'])
})

test('escape sequences', () => {
  expect(parseStringLiteral('"hello \\"world\\"".length')).toStrictEqual([{
    type: 'string-literal',
    quote: '"',
    content: 'hello "world"',
  } as StringLiteral, '.length'])
  expect(parseStringLiteral('"hello\\nworld".length')).toStrictEqual([{
    type: 'string-literal',
    quote: '"',
    content: 'hello\nworld',
  } as StringLiteral, '.length'])
  expect(parseStringLiteral('"C:\\\\hello\\\\world\\\\".length')).toStrictEqual([{
    type: 'string-literal',
    quote: '"',
    content: 'C:\\hello\\world\\',
  } as StringLiteral, '.length'])
})

test('unsupported escape sequences', () => {
  expect(() => parseStringLiteral('"hello \\x22world\\x22"')).toThrow(expect.objectContaining({
    code: 'ERR_PNPM_UNSUPPORTED_STRING_LITERAL_ESCAPE_SEQUENCE',
    sequence: 'x',
  }))
})

test('no closing quote', () => {
  expect(() => parseStringLiteral('"hello world')).toThrow(expect.objectContaining({
    code: 'ERR_PNPM_INCOMPLETE_STRING_LITERAL',
    expectedQuote: '"',
  }))
  expect(() => parseStringLiteral("'hello world")).toThrow(expect.objectContaining({
    code: 'ERR_PNPM_INCOMPLETE_STRING_LITERAL',
    expectedQuote: "'",
  }))
  expect(() => parseStringLiteral('"hello world\\"')).toThrow(expect.objectContaining({
    code: 'ERR_PNPM_INCOMPLETE_STRING_LITERAL',
    expectedQuote: '"',
  }))
  expect(() => parseStringLiteral("'hello world\\'")).toThrow(expect.objectContaining({
    code: 'ERR_PNPM_INCOMPLETE_STRING_LITERAL',
    expectedQuote: "'",
  }))
})
