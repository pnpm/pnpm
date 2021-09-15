import fs from 'fs'
import { dlx } from '@pnpm/plugin-commands-script-runners'
import { prepareEmpty } from '@pnpm/prepare'

test('dlx', async () => {
  prepareEmpty()

  await dlx.handler({}, ['shx', 'touch', 'foo'])

  expect(fs.existsSync('foo')).toBeTruthy()
})

test('dlx --package <pkg1> [--package <pkg2>]', async () => {
  prepareEmpty()

  await dlx.handler({
    package: [
      'zkochan/for-testing-pnpm-dlx',
      'is-positive',
    ],
  }, ['foo'])

  expect(fs.existsSync('foo')).toBeTruthy()
})
