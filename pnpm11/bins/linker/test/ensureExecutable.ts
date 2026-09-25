/// <reference path="../../../__typings__/index.d.ts"/>
import fs from 'node:fs'
import path from 'node:path'

import { beforeEach, expect, jest, test } from '@jest/globals'
import { fixtures } from '@pnpm/test-fixtures'
import isWindows from 'is-windows'
import { temporaryDirectory } from 'tempy'

const fixBinMock = jest.fn<(file: string, mode: number) => Promise<void>>()
jest.unstable_mockModule('bin-links/lib/fix-bin.js', () => ({
  default: fixBinMock,
}))

jest.unstable_mockModule('@pnpm/logger', () => ({
  logger: () => ({ debug: jest.fn() }),
  globalWarn: jest.fn(),
}))

const { linkBins } = await import('@pnpm/bins.linker')

const f = fixtures(import.meta.dirname)

beforeEach(() => {
  fixBinMock.mockReset()
})

// `fixBin` chmods the source file; on Windows there is no executable bit to assert
// against, so the read-only-store reasoning these tests cover does not apply.
const testOnPosix = isWindows() ? test.skip : test

testOnPosix('linkBins() skips fixBin when an executable source would reject chmod with EPERM', async () => {
  const eperm = Object.assign(new Error('EPERM: operation not permitted, chmod'), { code: 'EPERM' })
  fixBinMock.mockRejectedValue(eperm)

  const binTarget = temporaryDirectory()
  const fixture = f.prepare('simple-fixture')
  const binSource = path.join(fixture, 'node_modules', 'simple', 'index.js')
  // A complete seed already has executable bins, including on a read-only store.
  fs.chmodSync(binSource, 0o755)

  const warn = jest.fn()
  await expect(linkBins(path.join(fixture, 'node_modules'), binTarget, { warn })).resolves.toBeDefined()

  expect(fixBinMock).not.toHaveBeenCalled()
  expect(fs.existsSync(path.join(binTarget, 'simple'))).toBe(true)
})

testOnPosix('linkBins() rethrows EPERM from fixBin when the bin source is not executable', async () => {
  const eperm = Object.assign(new Error('EPERM: operation not permitted, chmod'), { code: 'EPERM' })
  fixBinMock.mockRejectedValue(eperm)

  const binTarget = temporaryDirectory()
  const fixture = f.prepare('simple-fixture')
  const binSource = path.join(fixture, 'node_modules', 'simple', 'index.js')
  // A broken seed: the bin is not executable, so the refused chmod is a real
  // problem and must surface rather than be silently swallowed.
  fs.chmodSync(binSource, 0o644)

  const warn = jest.fn()
  await expect(linkBins(path.join(fixture, 'node_modules'), binTarget, { warn })).rejects.toHaveProperty('code', 'EPERM')
})

testOnPosix('linkBins() skips fixBin when an executable source would reject chmod with EROFS', async () => {
  // A genuinely read-only filesystem (the primary frozenStore target) refuses
  // chmod with EROFS rather than EPERM/EACCES.
  const erofs = Object.assign(new Error('EROFS: read-only file system, chmod'), { code: 'EROFS' })
  fixBinMock.mockRejectedValue(erofs)

  const binTarget = temporaryDirectory()
  const fixture = f.prepare('simple-fixture')
  const binSource = path.join(fixture, 'node_modules', 'simple', 'index.js')
  fs.chmodSync(binSource, 0o755)

  const warn = jest.fn()
  await expect(linkBins(path.join(fixture, 'node_modules'), binTarget, { warn })).resolves.toBeDefined()

  expect(fixBinMock).not.toHaveBeenCalled()
  expect(fs.existsSync(path.join(binTarget, 'simple'))).toBe(true)
})

testOnPosix('linkBins() rethrows a chmod failure when the bin still has a CRLF shebang', async () => {
  // fixBin chmods *before* normalizing the shebang, so a chmod failure means the
  // CRLF was never rewritten. An executable-but-CRLF bin would not run on POSIX,
  // so the failure must surface even though the execute bit is set.
  const erofs = Object.assign(new Error('EROFS: read-only file system, chmod'), { code: 'EROFS' })
  fixBinMock.mockRejectedValue(erofs)

  const binTarget = temporaryDirectory()
  const fixture = f.prepare('simple-fixture')
  const binSource = path.join(fixture, 'node_modules', 'simple', 'index.js')
  fs.writeFileSync(binSource, '#!/usr/bin/env node\r\nconsole.log("hi")\n')
  fs.chmodSync(binSource, 0o755)

  const warn = jest.fn()
  await expect(linkBins(path.join(fixture, 'node_modules'), binTarget, { warn })).rejects.toHaveProperty('code', 'EROFS')
})

testOnPosix('linkBins() invokes fixBin when the bin source has only partial execute bits', async () => {
  const binTarget = temporaryDirectory()
  const fixture = f.prepare('simple-fixture')
  const binSource = path.join(fixture, 'node_modules', 'simple', 'index.js')
  fs.chmodSync(binSource, 0o744)

  const warn = jest.fn()
  await expect(linkBins(path.join(fixture, 'node_modules'), binTarget, { warn })).resolves.toBeDefined()

  expect(fixBinMock).toHaveBeenCalledWith(binSource, 0o755)
  expect(fs.existsSync(path.join(binTarget, 'simple'))).toBe(true)
})

testOnPosix('linkBins() rethrows EPERM from fixBin when the bin source has only partial execute bits', async () => {
  const eperm = Object.assign(new Error('EPERM: operation not permitted, chmod'), { code: 'EPERM' })
  fixBinMock.mockRejectedValue(eperm)

  const binTarget = temporaryDirectory()
  const fixture = f.prepare('simple-fixture')
  const binSource = path.join(fixture, 'node_modules', 'simple', 'index.js')
  fs.chmodSync(binSource, 0o744)

  const warn = jest.fn()
  await expect(linkBins(path.join(fixture, 'node_modules'), binTarget, { warn })).rejects.toHaveProperty('code', 'EPERM')
  expect(fixBinMock).toHaveBeenCalledWith(binSource, 0o755)
})

testOnPosix('linkBins() does not swallow non-ENOENT stat errors on already linked bins', async () => {
  const binTarget = temporaryDirectory()
  const fixture = f.prepare('simple-fixture')
  const warn = jest.fn()

  await linkBins(path.join(fixture, 'node_modules'), binTarget, { warn })

  const statSpy = jest.spyOn(fs.promises, 'stat').mockRejectedValueOnce(
    Object.assign(new Error('EACCES: permission denied, stat'), { code: 'EACCES' })
  )

  try {
    await expect(linkBins(path.join(fixture, 'node_modules'), binTarget, { warn })).rejects.toHaveProperty('code', 'EACCES')
  } finally {
    statSpy.mockRestore()
  }
})

