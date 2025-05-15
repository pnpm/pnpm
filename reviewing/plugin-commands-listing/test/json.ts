import { list } from '@pnpm/plugin-commands-listing'
import { prepare } from '@pnpm/prepare'
import { DEFAULT_OPTS } from './utils'

// Covers https://github.com/pnpm/pnpm/issues/8519
describe('correctly report the value of the private field when arguments are provided', () => {
  test.each([
    [undefined, false],
    [false, false],
    [true, true],
  ])('%s -> %s', async (given, expected) => {
    prepare({
      name: 'root',
      version: '0.0.0',
      private: given,
    })

    const output = await list.handler({
      ...DEFAULT_OPTS,
      dir: process.cwd(),
      json: true,
    }, ['root'])

    expect(JSON.parse(output)).toStrictEqual([{
      name: 'root',
      version: '0.0.0',
      private: expected,
      path: expect.any(String),
    }])
  })
})
