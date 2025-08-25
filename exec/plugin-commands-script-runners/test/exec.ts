import { prepareEmpty } from '@pnpm/prepare'
import { jest } from '@jest/globals'
import { DEFAULT_OPTS } from './utils/index.js'

jest.unstable_mockModule('execa', () => ({
  default: jest.fn(),
  sync: jest.fn(),
}))

const { default: execa } = await import('execa')
const { exec } = await import('@pnpm/plugin-commands-script-runners')

beforeEach(() => jest.mocked(execa).mockClear())

test('exec should set npm_config_user_agent', async () => {
  prepareEmpty()
  const userAgent = 'pnpm/0.0.0'

  await exec.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    selectedProjectsGraph: {},
    userAgent,
  }, ['eslint'])

  expect(execa).toHaveBeenCalledWith('eslint', [], expect.objectContaining({
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

  expect(execa).toHaveBeenCalledWith('eslint', [], expect.objectContaining({
    env: expect.objectContaining({
      NODE_OPTIONS: '--max-old-space-size=4096',
    }),
  }))
})

test('exec should specify the command', async () => {
  prepareEmpty()

  await expect(exec.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    selectedProjectsGraph: {},
  }, [])
  ).rejects.toThrow("'pnpm exec' requires a command to run")
})
