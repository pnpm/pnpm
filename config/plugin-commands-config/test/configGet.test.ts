import { config } from '@pnpm/plugin-commands-config'

test('config get --global', async () => {
  const configKey = await config.handler({
    dir: process.cwd(),
    configDir: process.cwd(),
    global: true,
    rawConfig: {
      'store-dir': '~/store',
    },
    rawLocalConfig: {},
  }, ['get', 'store-dir'])

  expect(configKey).toEqual('~/store')
})

test('config get', async () => {
  const configKey = await config.handler({
    dir: process.cwd(),
    configDir: process.cwd(),
    rawConfig: {},
    rawLocalConfig: {
      'fetch-retries': '2',
    },
  }, ['get', 'fetch-retries'])

  expect(configKey).toEqual('2')
})
