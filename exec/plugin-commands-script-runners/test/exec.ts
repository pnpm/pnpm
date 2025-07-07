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

test('exec should preserve existing NODE_OPTIONS in NODE_OPTIONS', async () => {
  prepareEmpty()
  const originalNodeOptions = process.env.NODE_OPTIONS
  process.env.NODE_OPTIONS = '--inspect-brk=9229'

  try {
    await exec.handler({
      ...DEFAULT_OPTS,
      dir: process.cwd(),
      selectedProjectsGraph: {},
      nodeOptions: '--max-old-space-size=4096',
    }, ['node', 'script.js'])

    expect(execa).toHaveBeenCalledWith('node', ['script.js'], expect.objectContaining({
      env: expect.objectContaining({
        NODE_OPTIONS: '--inspect-brk=9229 --max-old-space-size=4096',
      }),
    }))
  } finally {
    // Restore original NODE_OPTIONS
    if (originalNodeOptions !== undefined) {
      process.env.NODE_OPTIONS = originalNodeOptions
    } else {
      delete process.env.NODE_OPTIONS
    }
  }
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
