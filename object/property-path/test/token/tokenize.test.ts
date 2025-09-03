import { type Token, tokenize } from '../../src/index.js'

test('valid tokens', () => {
  expect(Array.from(tokenize(''))).toStrictEqual([] as Token[])
  expect(Array.from(tokenize(
    'packageExtensions.react.dependencies["@types/node"]'
  ))).toStrictEqual([
    { type: 'identifier', content: 'packageExtensions' },
    { type: 'exact', content: '.' },
    { type: 'identifier', content: 'react' },
    { type: 'exact', content: '.' },
    { type: 'identifier', content: 'dependencies' },
    { type: 'exact', content: '[' },
    { type: 'string-literal', quote: '"', content: '@types/node' },
    { type: 'exact', content: ']' },
  ] as Token[])
  expect(Array.from(tokenize(
    'packageExtensions  .react\n.dependencies[ "@types/node" ]'
  ))).toStrictEqual([
    { type: 'identifier', content: 'packageExtensions' },
    { type: 'whitespace' },
    { type: 'exact', content: '.' },
    { type: 'identifier', content: 'react' },
    { type: 'whitespace' },
    { type: 'exact', content: '.' },
    { type: 'identifier', content: 'dependencies' },
    { type: 'exact', content: '[' },
    { type: 'whitespace' },
    { type: 'string-literal', quote: '"', content: '@types/node' },
    { type: 'whitespace' },
    { type: 'exact', content: ']' },
  ] as Token[])
})

test('unexpected tokens', () => {
  expect(Array.from(tokenize('@'))).toStrictEqual([{ type: 'unexpected', content: '@' }] as Token[])
  expect(Array.from(tokenize(
    'packageExtensions.react.@!dependencies["@types/node"]'
  ))).toStrictEqual([
    { type: 'identifier', content: 'packageExtensions' },
    { type: 'exact', content: '.' },
    { type: 'identifier', content: 'react' },
    { type: 'exact', content: '.' },
    { type: 'unexpected', content: '@' },
    { type: 'unexpected', content: '!' },
    { type: 'identifier', content: 'dependencies' },
    { type: 'exact', content: '[' },
    { type: 'string-literal', quote: '"', content: '@types/node' },
    { type: 'exact', content: ']' },
  ] as Token[])
})
