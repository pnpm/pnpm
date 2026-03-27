import { jest } from '@jest/globals'
import { prepare } from '@pnpm/prepare'

import { DEFAULT_OPTS } from '../utils/index.js'

const installDeps = jest.fn()

jest.unstable_mockModule('../../src/installDeps.js', () => ({
  installDeps,
}))

const { handler } = await import('../../src/update/index.js')

beforeEach(() => {
  installDeps.mockReset()
})

test('retries latest updates without direct minimumReleaseAgeExclude matches after maturity failure', async () => {
  prepare({
    dependencies: {
      '@pnpm.e2e/foo': '^1.0.0',
      '@pnpm.e2e/bar': '^1.0.0',
    },
  })

  const err = Object.assign(new Error('maturity failure'), {
    code: 'ERR_PNPM_NO_MATURE_MATCHING_VERSION',
  })
  installDeps.mockRejectedValueOnce(err)
  installDeps.mockResolvedValueOnce(undefined)

  await handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    latest: true,
    minimumReleaseAgeExclude: ['@pnpm.e2e/foo', '@pnpm.e2e/bar@1.0.0'],
  }, ['@pnpm.e2e/foo'])

  expect(installDeps).toHaveBeenCalledTimes(2)
  expect(installDeps.mock.calls[1][0].minimumReleaseAgeExclude).toEqual(['@pnpm.e2e/bar@1.0.0'])
})

test('retries latest updates without scoped version-qualified direct exclusions after maturity failure', async () => {
  prepare({
    dependencies: {
      '@pnpm.e2e/foo': '^1.0.0',
    },
  })

  const err = Object.assign(new Error('maturity failure'), {
    code: 'ERR_PNPM_NO_MATURE_MATCHING_VERSION',
  })
  installDeps.mockRejectedValueOnce(err)
  installDeps.mockResolvedValueOnce(undefined)

  await handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    latest: true,
    minimumReleaseAgeExclude: ['@pnpm.e2e/foo@1.0.0'],
  }, ['@pnpm.e2e/foo'])

  expect(installDeps).toHaveBeenCalledTimes(2)
  expect(installDeps.mock.calls[1][0].minimumReleaseAgeExclude).toEqual([])
})

test('full latest updates keep exclusions for ignored direct dependencies on retry', async () => {
  prepare({
    dependencies: {
      '@pnpm.e2e/foo': '^1.0.0',
      '@pnpm.e2e/bar': '^1.0.0',
    },
  })

  const err = Object.assign(new Error('maturity failure'), {
    code: 'ERR_PNPM_NO_MATURE_MATCHING_VERSION',
  })
  installDeps.mockRejectedValueOnce(err)
  installDeps.mockResolvedValueOnce(undefined)

  await handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    latest: true,
    minimumReleaseAgeExclude: ['@pnpm.e2e/foo', '@pnpm.e2e/bar'],
    updateConfig: {
      ignoreDependencies: ['@pnpm.e2e/bar'],
    },
  }, [])

  expect(installDeps).toHaveBeenCalledTimes(2)
  expect(installDeps.mock.calls[1][0].minimumReleaseAgeExclude).toEqual(['@pnpm.e2e/bar'])
})
