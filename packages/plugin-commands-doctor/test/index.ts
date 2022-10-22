import { doctor } from '@pnpm/plugin-commands-doctor'
import { logger } from '@pnpm/logger'

beforeEach(() => {
  jest.spyOn(logger, 'warn')
})

afterEach(() => {
  (logger.warn as jest.Mock).mockRestore()
})

test('doctor', async () => {
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

  // In the scope of jest, require.resolve.paths('npm') cannot reach global npm path by default
  await doctor.handler({
    failedToLoadBuiltInConfig: true,
  })

  expect(logger.warn).toHaveBeenCalledWith({
    message: expect.stringMatching(/^Load npm builtin configs failed./),
    prefix: process.cwd(),
  })

  process.env = oldEnv
  process.execPath = oldExecPath
  Object.defineProperty(process, 'platform', {
    value: oldPlatform,
  })
})
