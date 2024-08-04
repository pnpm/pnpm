import path from 'path'
import { prependDirsToPath } from '@pnpm/env.path'
import PATH from 'path-name'

test('prependDirsToPath', () => {
  expect(prependDirsToPath(['foo'], {})).toStrictEqual({
    name: PATH,
    value: 'foo',
  })
  expect(prependDirsToPath(['foo'], { [PATH]: 'bar' })).toStrictEqual({
    name: PATH,
    value: `foo${path.delimiter}bar`,
  })
})
