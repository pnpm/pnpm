import fs from 'fs'
import path from 'path'
import { tempDir } from '@pnpm/prepare'
import { env } from '@pnpm/plugin-commands-env'
import execa from 'execa'
import PATH from 'path-name'

test('install node', async () => {
  tempDir()

  await env.handler({
    bin: process.cwd(),
    global: true,
    pnpmHomeDir: process.cwd(),
    rawConfig: {},
  }, ['use', '16.4.0'])

  const { stdout } = execa.sync('node', ['-v'], {
    env: {
      [PATH]: `${process.cwd()}${path.delimiter}${process.env[PATH] as string}`,
    },
  })
  expect(stdout.toString()).toBe('v16.4.0')

  const dirs = fs.readdirSync(path.resolve('nodejs'))
  expect(dirs).toEqual(['16.4.0'])
})
