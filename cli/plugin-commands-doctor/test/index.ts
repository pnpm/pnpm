import { jest } from '@jest/globals'
import { logger } from '@pnpm/logger'
import { doctor } from '@pnpm/plugin-commands-doctor'

beforeEach(() => {
  jest.spyOn(logger, 'warn')
})

afterEach(() => {
  jest.mocked(logger.warn).mockRestore()
})

test('doctor', async () => {
  // In the scope of jest, require.resolve.paths('npm') cannot reach global npm path by default
  await doctor.handler({
    failedToLoadBuiltInConfig: true,
  })

  expect(logger.warn).toHaveBeenCalledWith({
    message: expect.stringMatching(/^Load npm builtin configs failed./),
    prefix: process.cwd(),
  })
})
