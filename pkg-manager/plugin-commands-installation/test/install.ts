import fs from 'fs'
import delay from 'delay'
import path from 'path'
import { add, install } from '@pnpm/plugin-commands-installation'
import { prepareEmpty } from '@pnpm/prepare'
import rimraf from '@zkochan/rimraf'
import { DEFAULT_OPTS } from './utils'

test('install fails if no package.json is found', async () => {
  prepareEmpty()

  await expect(install.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
  })).rejects.toThrow(/No package\.json found/)
})

test('install does not fail when a new package is added', async () => {
  prepareEmpty()

  await add.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
  }, ['is-positive@1.0.0'])

  const pkg = await import(path.resolve('package.json'))

  expect(pkg?.dependencies).toStrictEqual({ 'is-positive': '1.0.0' })
})

test('install with no store integrity validation', async () => {
  prepareEmpty()

  await add.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
  }, ['is-positive@1.0.0'])

  // We should have a short delay before modifying the file in the store.
  // Otherwise pnpm will not consider it to be modified.
  await delay(200)
  const readmePath = path.join(DEFAULT_OPTS.storeDir, 'v3/files/9a/f6af85f55c111108eddf1d7ef7ef224b812e7c7bfabae41c79cf8bc9a910352536963809463e0af2799abacb975f22418a35a1d170055ef3fdc3b2a46ef1c5')
  fs.writeFileSync(readmePath, 'modified', 'utf8')

  await rimraf('node_modules')

  await install.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    verifyStoreIntegrity: false,
  })

  expect(fs.readFileSync('node_modules/is-positive/readme.md', 'utf8')).toBe('modified')
})
