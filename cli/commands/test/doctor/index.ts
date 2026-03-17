import { jest } from '@jest/globals'
import { doctor } from '@pnpm/cli.commands'
import { logger } from '@pnpm/logger'

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
