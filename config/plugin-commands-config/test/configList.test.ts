import * as ini from 'ini'
import { config } from '@pnpm/plugin-commands-config'
import { getOutputString, DEFAULT_OPTS } from './utils/index.js'

test('config list', async () => {
  const output = await config.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
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
    ...DEFAULT_OPTS,
    dir: process.cwd(),
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
