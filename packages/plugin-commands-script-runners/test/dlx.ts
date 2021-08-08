import fs from 'fs'
import { dlx } from '@pnpm/plugin-commands-script-runners'
import { prepareEmpty } from '@pnpm/prepare'

test('dlx', async () => {
  prepareEmpty()

  await dlx.handler({}, ['touch', 'foo'])

  expect(fs.existsSync('foo')).toBeTruthy()
})

test('dlx --package <pkg>', async () => {
  prepareEmpty()

  await dlx.handler({
    package: 'zkochan/for-testing-pnpm-dlx',
  }, ['foo'])

  expect(fs.existsSync('foo')).toBeTruthy()
})
