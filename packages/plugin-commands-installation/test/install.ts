import path from 'path'
import { add, install } from '@pnpm/plugin-commands-installation'
import { prepareEmpty } from '@pnpm/prepare'
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
