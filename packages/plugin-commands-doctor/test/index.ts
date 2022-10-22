import { doctor } from '@pnpm/plugin-commands-doctor'
import { logger } from '@pnpm/logger'

beforeEach(() => {
  jest.spyOn(logger, 'warn')
})

afterEach(() => {
  (logger.warn as jest.Mock).mockRestore()
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
