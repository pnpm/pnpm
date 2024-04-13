import { config } from '@pnpm/plugin-commands-config'

test('config get', async () => {
  const configKey = await config.handler({
    dir: process.cwd(),
    cliOptions: {},
    configDir: process.cwd(),
    global: true,
    rawConfig: {
      'store-dir': '~/store',
    },
  }, ['get', 'store-dir'])

  expect(configKey).toEqual('~/store')
})

test('config get a boolean should return string format', async () => {
  const configKey = await config.handler({
    dir: process.cwd(),
    cliOptions: {},
    configDir: process.cwd(),
    global: true,
    rawConfig: {
      'update-notifier': true,
    },
  }, ['get', 'update-notifier'])

  expect(configKey).toEqual('true')
})

test('config get on array should return a comma-separated list', async () => {
  const configKey = await config.handler({
    dir: process.cwd(),
    cliOptions: {},
    configDir: process.cwd(),
    global: true,
    rawConfig: {
      'public-hoist-pattern': [
        '*eslint*',
        '*prettier*',
      ],
    },
  }, ['get', 'public-hoist-pattern'])

  expect(configKey).toBe('*eslint*,*prettier*')
})

test('config get without key show list all settings ', async () => {
  const rawConfig = {
    'store-dir': '~/store',
    'fetch-retries': '2',
  }
  const getOutput = await config.handler({
    dir: process.cwd(),
    cliOptions: {},
    configDir: process.cwd(),
    global: true,
    rawConfig,
  }, ['get'])

  const listOutput = await config.handler({
    dir: process.cwd(),
    cliOptions: {},
    configDir: process.cwd(),
    rawConfig,
  }, ['list'])

  expect(getOutput).toEqual(listOutput)
})
