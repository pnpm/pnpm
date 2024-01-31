import execa from 'execa'
import { exec } from '@pnpm/plugin-commands-script-runners'
import { prepareEmpty } from '@pnpm/prepare'
import { DEFAULT_OPTS } from './utils'

jest.mock('execa')

beforeEach((execa as jest.Mock).mockClear)

test('exec should set npm_config_user_agent', async () => {
  prepareEmpty()
  const userAgent = 'pnpm/0.0.0'

  await exec.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    selectedProjectsGraph: {},
    userAgent,
  }, ['eslint'])

  expect(execa).toBeCalledWith('eslint', [], expect.objectContaining({
    env: expect.objectContaining({
      npm_config_user_agent: userAgent,
    }),
  }))
})

test('exec should set the NODE_OPTIONS env var', async () => {
  prepareEmpty()

  await exec.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    selectedProjectsGraph: {},
    nodeOptions: '--max-old-space-size=4096',
  }, ['eslint'])

  expect(execa).toBeCalledWith('eslint', [], expect.objectContaining({
    env: expect.objectContaining({
      NODE_OPTIONS: '--max-old-space-size=4096',
    }),
  }))
})
