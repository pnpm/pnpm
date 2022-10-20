import { doctor } from '@pnpm/plugin-commands-audit'
import sinon from 'sinon'
import { logger } from '@pnpm/logger'

test('doctor --config', async () => {
  const oldExecPath = process.execPath
  const oldPlatform = process.platform
  const oldEnv = process.env
  const HOMEBREW_PREFIX = '.'

  process.env = { ...oldEnv, HOMEBREW_PREFIX }
  process.execPath = HOMEBREW_PREFIX
  // platform is read only
  Object.defineProperty(process, 'platform', {
    value: 'darwin',
  })

  const reporter = sinon.spy(logger, 'warn')

  // In the scope of jest, require.resolve.paths('npm') cannot reach global npm path by default
  await doctor.handler({
    config: true,
  })

  expect(reporter.calledWithMatch({
    message: 'Load npm builtin configs failed. If the prefix builtin config does not work, you can use "pnpm config ls" to show builtin configs. And then use "pnpm config --global set <key> <value>" to migrate configs from builtin to global.',
    prefix: process.cwd(),
  })).toBeTruthy()

  process.env = oldEnv
  process.execPath = oldExecPath
  Object.defineProperty(process, 'platform', {
    value: oldPlatform,
  })
})