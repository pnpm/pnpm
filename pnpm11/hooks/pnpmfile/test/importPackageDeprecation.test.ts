import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { beforeEach, expect, jest, test } from '@jest/globals'

jest.unstable_mockModule('@pnpm/logger', () => ({
  globalWarn: jest.fn(),
  logger: () => ({ debug: jest.fn() }),
}))

const { globalWarn } = await import('@pnpm/logger')
const { requireHooks } = await import('../lib/requireHooks.js')

const testOnPosix = process.platform === 'win32' ? test.skip : test

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
  expect(warning).toContain('will be removed in the next major version')
  expect(warning).toContain('keeps working until then')
  expect(warning).toContain('parallel package importer')
})

test('requireHooks() does not warn when no importPackage hook is defined', async () => {
  const pnpmfile = path.join(import.meta.dirname, '__fixtures__/filterLog.js')
  await requireHooks(import.meta.dirname, { pnpmfiles: [pnpmfile] })

  expect(globalWarn).not.toHaveBeenCalled()
})

testOnPosix('requireHooks() strips control characters from the pnpmfile path in the warning', async () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpmfile-'))
  const pnpmfile = path.join(dir, 'spoofed\u001b[2K\nnot-really-a-hook.pnpmfile.cjs')
  fs.copyFileSync(path.join(import.meta.dirname, '__fixtures__/importPackage.js'), pnpmfile)

  try {
    await requireHooks(dir, { pnpmfiles: [pnpmfile] })
  } finally {
    fs.rmSync(dir, { recursive: true, force: true })
  }

  const warning = jest.mocked(globalWarn).mock.calls[0][0]
  expect(warning).not.toContain('\u001b')
  expect(warning).not.toContain('\n')
  expect(warning).toContain('not-really-a-hook.pnpmfile.cjs')
})
