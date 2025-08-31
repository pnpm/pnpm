import { type NumericLiteral, parseNumericLiteral } from '../../src/index.js'

test('not a numeric literal', () => {
  expect(parseNumericLiteral('')).toBeUndefined()
  expect(parseNumericLiteral('abcdef')).toBeUndefined()
  expect(parseNumericLiteral('"hello world"')).toBeUndefined()
  expect(parseNumericLiteral('.123')).toBeUndefined()
  expect(parseNumericLiteral('NaN')).toBeUndefined()
})

test('simple numbers', () => {
  expect(parseNumericLiteral('0')).toStrictEqual([{
    type: 'numeric-literal',
    content: 0,
  } as NumericLiteral, ''])
  expect(parseNumericLiteral('3')).toStrictEqual([{
    type: 'numeric-literal',
    content: 3,
  } as NumericLiteral, ''])
  expect(parseNumericLiteral('123')).toStrictEqual([{
    type: 'numeric-literal',
    content: 123,
  } as NumericLiteral, ''])
  expect(parseNumericLiteral('123.4')).toStrictEqual([{
    type: 'numeric-literal',
    content: 123.4,
  } as NumericLiteral, ''])
  expect(parseNumericLiteral('0123')).toStrictEqual([{
    type: 'numeric-literal',
    content: 123,
  } as NumericLiteral, ''])
  expect(parseNumericLiteral('123,456')).toStrictEqual([{
    type: 'numeric-literal',
    content: 123,
  } as NumericLiteral, ',456'])
})

test('unsupported syntax', () => {
  expect(() => parseNumericLiteral('0x12AB')).toThrow(expect.objectContaining({
    code: 'ERR_PNPM_UNSUPPORTED_NUMERIC_LITERAL_SUFFIX',
    suffix: 'x',
  }))
  expect(() => parseNumericLiteral('1e23')).toThrow(expect.objectContaining({
    code: 'ERR_PNPM_UNSUPPORTED_NUMERIC_LITERAL_SUFFIX',
    suffix: 'e',
  }))
  expect(() => parseNumericLiteral('123n')).toThrow(expect.objectContaining({
    code: 'ERR_PNPM_UNSUPPORTED_NUMERIC_LITERAL_SUFFIX',
    suffix: 'n',
  }))
  expect(() => parseNumericLiteral('123ABC')).toThrow(expect.objectContaining({
    code: 'ERR_PNPM_UNSUPPORTED_NUMERIC_LITERAL_SUFFIX',
    suffix: 'A',
  }))
})
