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
