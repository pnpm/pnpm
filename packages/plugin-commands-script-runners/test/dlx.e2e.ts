import fs from 'fs'
import { dlx } from '@pnpm/plugin-commands-script-runners'
import { prepareEmpty } from '@pnpm/prepare'
import { DLX_DEFAULT_OPTS as DEFAULT_OPTS } from './utils'

test('dlx', async () => {
  prepareEmpty()

  await dlx.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
  }, ['shx', 'touch', 'foo'])

  expect(fs.existsSync('foo')).toBeTruthy()
})

test('dlx should work when the package name differs from the bin name', async () => {
  prepareEmpty()

  await dlx.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
  }, ['touch-file-one-bin'])

  expect(fs.existsSync('touch.txt')).toBeTruthy()
})

test('dlx --package <pkg1> [--package <pkg2>]', async () => {
  prepareEmpty()

  await dlx.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    package: [
      'zkochan/for-testing-pnpm-dlx',
      'is-positive',
    ],
  }, ['foo'])

  expect(fs.existsSync('foo')).toBeTruthy()
})
