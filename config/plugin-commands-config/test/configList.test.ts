import { config } from '@pnpm/plugin-commands-config'

const CRLF = '\r\n'

function normalizeNewlines (str: string) {
  return str.replace(new RegExp(CRLF, 'g'), '\n')
}

test('config list', async () => {
  const output = await config.handler({
    dir: process.cwd(),
    configDir: process.cwd(),
    rawConfig: {
      'store-dir': '~/store',
      'fetch-retries': '2',
    },
  }, ['list'])

  expect(normalizeNewlines(output)).toEqual(`fetch-retries=2
store-dir=~/store
`)
})

test('config list --json', async () => {
  const output = await config.handler({
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
