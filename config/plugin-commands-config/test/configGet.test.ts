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
