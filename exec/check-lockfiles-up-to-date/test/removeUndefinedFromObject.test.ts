import { removeUndefinedFromObject } from '../src/removeUndefinedFromObject'

test.each([
  [undefined, undefined],
  [{}, undefined],
  [null, null],
  [{ a: undefined, b: {} }, undefined],
  [{ a: undefined, b: {}, c: 'foo', d: { e: undefined } }, { c: 'foo' }],
  [[undefined, {}, { a: undefined }, { a: {} }, { a: 0, b: 1 }], [undefined, undefined, undefined, undefined, { a: 0, b: 1 }]],
  ['foo', 'foo'],
  [() => {}, expect.any(Function)],
])('%p → %p', (input, output) => {
  expect(removeUndefinedFromObject(input)).toStrictEqual(output)
})
