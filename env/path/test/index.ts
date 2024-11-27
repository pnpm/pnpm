import path from 'path'
import { prependDirsToPath } from '@pnpm/env.path'
import PATH from 'path-name'

test('prependDirsToPath', () => {
  expect(prependDirsToPath(['foo'], {})).toStrictEqual({
    name: PATH,
    value: 'foo',
    updated: true,
  })
  expect(prependDirsToPath(['foo'], { [PATH]: 'bar' })).toStrictEqual({
    name: PATH,
    value: `foo${path.delimiter}bar`,
    updated: true,
  })
  expect(prependDirsToPath(['foo', 'qar'], { [PATH]: `foo${path.delimiter}qar${path.delimiter}bar` })).toStrictEqual({
    name: PATH,
    value: `foo${path.delimiter}qar${path.delimiter}bar`,
    updated: false,
  })
  expect(prependDirsToPath(['foo', 'qar'], { [PATH]: `foo${path.delimiter}qar` })).toStrictEqual({
    name: PATH,
    value: `foo${path.delimiter}qar`,
    updated: false,
  })
})
