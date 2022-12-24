import { config } from '@pnpm/plugin-commands-config'

test('config get', async () => {
  const configKey = await config.handler({
    dir: process.cwd(),
    configDir: process.cwd(),
    global: true,
    rawConfig: {
      'store-dir': '~/store',
    },
  }, ['get', 'store-dir'])

  expect(configKey).toEqual('~/store')
})
