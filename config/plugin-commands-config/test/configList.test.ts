import * as ini from 'ini'
import { config } from '@pnpm/plugin-commands-config'
import { getOutputString } from './utils'

test('config list', async () => {
  const output = await config.handler({
    dir: process.cwd(),
    cliOptions: {},
    configDir: process.cwd(),
    rawConfig: {
      'store-dir': '~/store',
      'fetch-retries': '2',
    },
  }, ['list'])

  expect(ini.decode(getOutputString(output))).toEqual({
    'fetch-retries': '2',
    'store-dir': '~/store',
  })
})

test('config list --json', async () => {
  const output = await config.handler({
    dir: process.cwd(),
    cliOptions: {},
    configDir: process.cwd(),
    json: true,
    rawConfig: {
      'store-dir': '~/store',
      'fetch-retries': '2',
    },
  }, ['list'])

  expect(output).toEqual(JSON.stringify({
    'fetch-retries': '2',
    'store-dir': '~/store',
  }, null, 2))
})
