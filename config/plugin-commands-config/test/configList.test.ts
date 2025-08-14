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

test('config list censors protected settings', async () => {
  const rawConfig = {
    'store-dir': '~/store',
    'fetch-retries': '2',
    'username': 'general-username',
    '@my-org:registry': 'https://my-org.example.com/registry',
    '//my-org.example.com:username': 'my-username-in-my-org',
  }

  const output = await config.handler({
    dir: process.cwd(),
    cliOptions: {},
    configDir: process.cwd(),
    rawConfig,
  }, ['list'])

  expect(ini.decode(getOutputString(output))).toEqual({
    ...rawConfig,
    '//my-org.example.com:username': '(protected)',
    username: '(protected)',
  })
})

test('config list --json censors protected settings', async () => {
  const rawConfig = {
    'store-dir': '~/store',
    'fetch-retries': '2',
    'username': 'general-username',
    '@my-org:registry': 'https://my-org.example.com/registry',
    '//my-org.example.com:username': 'my-username-in-my-org',
  }

  const output = await config.handler({
    dir: process.cwd(),
    json: true,
    cliOptions: {},
    configDir: process.cwd(),
    rawConfig,
  }, ['list'])

  expect(JSON.parse(getOutputString(output))).toStrictEqual({
    ...rawConfig,
    '//my-org.example.com:username': '(protected)',
    username: '(protected)',
  })
})
