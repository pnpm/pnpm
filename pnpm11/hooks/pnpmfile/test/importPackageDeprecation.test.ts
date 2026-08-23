import path from 'node:path'

import { beforeEach, expect, jest, test } from '@jest/globals'

jest.unstable_mockModule('@pnpm/logger', () => ({
  globalWarn: jest.fn(),
  logger: () => ({ debug: jest.fn() }),
}))

const { globalWarn } = await import('@pnpm/logger')
const { requireHooks } = await import('../lib/requireHooks.js')

beforeEach(() => {
  jest.mocked(globalWarn).mockClear()
})

test('requireHooks() warns that the importPackage hook is deprecated', async () => {
  const pnpmfile = path.join(import.meta.dirname, '__fixtures__/importPackage.js')
  const { hooks } = await requireHooks(import.meta.dirname, { pnpmfiles: [pnpmfile] })

  expect(hooks.importPackage).toBeDefined()
  expect(globalWarn).toHaveBeenCalledTimes(1)
  const warning = jest.mocked(globalWarn).mock.calls[0][0]
  expect(warning).toContain('"importPackage" hook')
  expect(warning).toContain(pnpmfile)
  expect(warning).toContain('deprecated')
})

test('requireHooks() does not warn when no importPackage hook is defined', async () => {
  const pnpmfile = path.join(import.meta.dirname, '__fixtures__/filterLog.js')
  await requireHooks(import.meta.dirname, { pnpmfiles: [pnpmfile] })

  expect(globalWarn).not.toHaveBeenCalled()
})
