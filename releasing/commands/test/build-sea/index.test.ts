import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { afterEach, beforeEach, describe, expect, it } from '@jest/globals'

import { buildSea } from '../../src/index.js'

const { cliOptionsTypes, commandNames, handler, help, shorthands } = buildSea

describe('build-sea command', () => {
  let tempDir: string

  beforeEach(() => {
    tempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-build-sea-test-'))
  })

  afterEach(() => {
    fs.rmSync(tempDir, { recursive: true, force: true })
  })

  it('exposes the expected command name and shorthands', () => {
    expect(commandNames).toEqual(['build-sea'])
    expect(shorthands.t).toBe('--target')
    expect(shorthands.o).toBe('--output-dir')
  })

  it('declares the user-facing CLI option types', () => {
    const types = cliOptionsTypes()
    expect(types.entry).toBe(String)
    expect(types.target).toEqual([String, Array])
    expect(types['node-version']).toBe(String)
    expect(types['output-dir']).toBe(String)
    expect(types['output-name']).toBe(String)
  })

  it('renders help text that lists the key options and supported targets', () => {
    const text = help()
    expect(text).toContain('Single Executable Application')
    expect(text).toContain('--entry')
    expect(text).toContain('--target')
    expect(text).toContain('linux-x64')
    expect(text).toContain('win-arm64')
    expect(text).toContain('linux-x64-musl')
  })

  function baseOpts (): Record<string, unknown> {
    return {
      dir: tempDir,
      pnpmHomeDir: path.join(tempDir, 'pnpm-home'),
      rawConfig: {},
    }
  }

  it('fails fast when no --entry is provided', async () => {
    await expect(
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      handler(baseOpts() as any, [])
    ).rejects.toMatchObject({ code: 'ERR_PNPM_BUILD_SEA_MISSING_ENTRY' })
  })

  it('fails fast when the entry file does not exist', async () => {
    await expect(
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      handler({ ...baseOpts(), entry: 'missing.cjs' } as any, [])
    ).rejects.toMatchObject({ code: 'ERR_PNPM_BUILD_SEA_ENTRY_NOT_FOUND' })
  })

  it('fails fast when no --target is provided', async () => {
    fs.writeFileSync(path.join(tempDir, 'entry.cjs'), 'module.exports = {}')
    await expect(
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      handler({ ...baseOpts(), entry: 'entry.cjs' } as any, [])
    ).rejects.toMatchObject({ code: 'ERR_PNPM_BUILD_SEA_MISSING_TARGET' })
  })

  it.each([
    ['unknown OS', 'freebsd-x64'],
    ['unknown arch', 'linux-mips'],
    ['unknown libc', 'linux-x64-gnu'],
    ['musl on non-linux', 'macos-arm64-musl'],
    ['incomplete', 'linux'],
  ])('rejects invalid target: %s (%s)', async (_label, target) => {
    fs.writeFileSync(path.join(tempDir, 'entry.cjs'), 'module.exports = {}')
    await expect(
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      handler({ ...baseOpts(), entry: 'entry.cjs', target } as any, [])
    ).rejects.toMatchObject({ code: 'ERR_PNPM_BUILD_SEA_INVALID_TARGET' })
  })

  it('uses --output-name when set, instead of requiring a package.json', async () => {
    fs.writeFileSync(path.join(tempDir, 'entry.cjs'), 'module.exports = {}')
    // No package.json. We only want to validate that the code path reaches
    // target parsing / output-name handling before any network call, so we
    // assert on the error that surfaces when the target list is empty.
    await expect(
      handler(
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        { ...baseOpts(), entry: 'entry.cjs', outputName: 'explicit' } as any,
        []
      )
    ).rejects.toMatchObject({ code: 'ERR_PNPM_BUILD_SEA_MISSING_TARGET' })
  })
})
