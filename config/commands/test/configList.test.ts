import { expect, test } from '@jest/globals'
import { config } from '@pnpm/config.commands'

import { createConfigCommandOpts, getOutputString } from './utils/index.js'

test('config list', async () => {
  const output = await config.handler(createConfigCommandOpts({
    dir: process.cwd(),
    cliOptions: {},
    configDir: process.cwd(),
    authConfig: {},
    storeDir: '~/store',
    fetchRetries: '2',
  }), ['list'])

  expect(JSON.parse(getOutputString(output))).toMatchObject({
    fetchRetries: '2',
    storeDir: '~/store',
  })
})

test('config list --json', async () => {
  const output = await config.handler(createConfigCommandOpts({
    dir: process.cwd(),
    cliOptions: {},
    configDir: process.cwd(),
    json: true,
    authConfig: {},
    storeDir: '~/store',
    fetchRetries: '2',
  }), ['list'])

  const parsed = JSON.parse(output as string)
  expect(parsed).toMatchObject({
    fetchRetries: '2',
    storeDir: '~/store',
  })
})

test('config list censors protected settings', async () => {
  const authConfig = {
    username: 'general-username',
    '@my-org:registry': 'https://my-org.example.com/registry',
    '//my-org.example.com:username': 'my-username-in-my-org',
  }

  const output = await config.handler(createConfigCommandOpts({
    dir: process.cwd(),
    cliOptions: {},
    configDir: process.cwd(),
    storeDir: '~/store',
    fetchRetries: '2',
    authConfig,
  }), ['list'])

  expect(JSON.parse(getOutputString(output))).toMatchObject({
    storeDir: '~/store',
    fetchRetries: '2',
    '@my-org:registry': 'https://my-org.example.com/registry',
    '//my-org.example.com:username': '(protected)',
    username: '(protected)',
  })
})

test('config list --json censors protected settings', async () => {
  const authConfig = {
    username: 'general-username',
    '@my-org:registry': 'https://my-org.example.com/registry',
    '//my-org.example.com:username': 'my-username-in-my-org',
  }

  const output = await config.handler(createConfigCommandOpts({
    dir: process.cwd(),
    json: true,
    cliOptions: {},
    configDir: process.cwd(),
    storeDir: '~/store',
    fetchRetries: '2',
    authConfig,
  }), ['list'])

  expect(JSON.parse(getOutputString(output))).toMatchObject({
    storeDir: '~/store',
    fetchRetries: '2',
    username: '(protected)',
    '@my-org:registry': 'https://my-org.example.com/registry',
    '//my-org.example.com:username': '(protected)',
  })
})
