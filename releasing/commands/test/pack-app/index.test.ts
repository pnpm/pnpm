import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { afterEach, beforeEach, describe, expect, it } from '@jest/globals'

import { packApp } from '../../src/index.js'

const { cliOptionsTypes, commandNames, handler, help, shorthands } = packApp

describe('pack-app command', () => {
  let tempDir: string

  beforeEach(() => {
    tempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-pack-app-test-'))
  })

  afterEach(() => {
    fs.rmSync(tempDir, { recursive: true, force: true })
  })

  it('exposes the expected command name and shorthands', () => {
    expect(commandNames).toEqual(['pack-app'])
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
    expect(text).toContain('win32-arm64')
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
    ).rejects.toMatchObject({ code: 'ERR_PNPM_PACK_APP_MISSING_ENTRY' })
  })

  it('fails fast when the entry file does not exist', async () => {
    await expect(
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      handler({ ...baseOpts(), entry: 'missing.cjs' } as any, [])
    ).rejects.toMatchObject({ code: 'ERR_PNPM_PACK_APP_ENTRY_NOT_FOUND' })
  })

  it('fails fast when the entry path is a directory', async () => {
    fs.mkdirSync(path.join(tempDir, 'entry-dir'))
    await expect(
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      handler({ ...baseOpts(), entry: 'entry-dir' } as any, [])
    ).rejects.toMatchObject({ code: 'ERR_PNPM_PACK_APP_ENTRY_NOT_FILE' })
  })

  it('reads entry from pnpm.app.entry when --entry is omitted', async () => {
    fs.writeFileSync(path.join(tempDir, 'package.json'), JSON.stringify({
      name: 'test-app',
      pnpm: { app: { entry: 'from-config.cjs' } },
    }))
    fs.writeFileSync(path.join(tempDir, 'from-config.cjs'), 'module.exports = {}')
    // With entry from config but no target, we hit MISSING_TARGET — that's
    // enough to verify the entry was picked up from pnpm.app.entry.
    await expect(
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      handler(baseOpts() as any, [])
    ).rejects.toMatchObject({ code: 'ERR_PNPM_PACK_APP_MISSING_TARGET' })
  })

  it('reads targets from pnpm.app.targets when --target is omitted', async () => {
    fs.writeFileSync(path.join(tempDir, 'package.json'), JSON.stringify({
      name: 'test-app',
      pnpm: { app: { targets: ['bad-target'] } },
    }))
    fs.writeFileSync(path.join(tempDir, 'entry.cjs'), 'module.exports = {}')
    // A bad-target in the config should reach parseTarget and surface
    // INVALID_TARGET — proves the config list was consulted.
    await expect(
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      handler({ ...baseOpts(), entry: 'entry.cjs' } as any, [])
    ).rejects.toMatchObject({ code: 'ERR_PNPM_PACK_APP_INVALID_TARGET' })
  })

  it('CLI --target replaces pnpm.app.targets entirely (no merging)', async () => {
    // Config says targets = [bad-target]. If the CLI list were merged in, the
    // bad config entry would still hit parseTarget and throw INVALID_TARGET.
    // With an unresolvable node version, validation passes but the later
    // version lookup fails — we only assert that INVALID_TARGET never fires.
    fs.writeFileSync(path.join(tempDir, 'package.json'), JSON.stringify({
      name: 'test-app',
      pnpm: { app: { entry: 'entry.cjs', targets: ['bad-target'] } },
    }))
    fs.writeFileSync(path.join(tempDir, 'entry.cjs'), 'module.exports = {}')
    await expect(
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      handler({ ...baseOpts(), target: 'linux-x64', nodeVersion: '0.0.0-nonexistent-xxx' } as any, [])
    ).rejects.toMatchObject({ code: expect.not.stringMatching(/INVALID_TARGET/) })
  })

  it('rejects unknown keys in pnpm.app', async () => {
    fs.writeFileSync(path.join(tempDir, 'package.json'), JSON.stringify({
      name: 'test-app',
      pnpm: { app: { entry: 'entry.cjs', bogus: 'yes' } },
    }))
    fs.writeFileSync(path.join(tempDir, 'entry.cjs'), 'module.exports = {}')
    await expect(
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      handler(baseOpts() as any, [])
    ).rejects.toMatchObject({ code: 'ERR_PNPM_PACK_APP_INVALID_CONFIG' })
  })

  it.each([
    ['entry as number', { entry: 42 }],
    ['targets as string', { targets: 'linux-x64' }],
    ['targets with non-string', { targets: ['linux-x64', 7] }],
    ['nodeVersion as array', { nodeVersion: ['25'] }],
  ])('rejects malformed pnpm.app: %s', async (_label, appConfig) => {
    fs.writeFileSync(path.join(tempDir, 'package.json'), JSON.stringify({
      name: 'test-app',
      pnpm: { app: appConfig },
    }))
    fs.writeFileSync(path.join(tempDir, 'entry.cjs'), 'module.exports = {}')
    await expect(
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      handler(baseOpts() as any, [])
    ).rejects.toMatchObject({ code: 'ERR_PNPM_PACK_APP_INVALID_CONFIG' })
  })

  it('fails fast when no --target is provided', async () => {
    fs.writeFileSync(path.join(tempDir, 'entry.cjs'), 'module.exports = {}')
    await expect(
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      handler({ ...baseOpts(), entry: 'entry.cjs' } as any, [])
    ).rejects.toMatchObject({ code: 'ERR_PNPM_PACK_APP_MISSING_TARGET' })
  })

  it.each([
    ['unknown OS', 'freebsd-x64'],
    ['unknown arch', 'linux-mips'],
    ['unknown libc', 'linux-x64-gnu'],
    ['musl on non-linux', 'darwin-arm64-musl'],
    ['legacy macos alias', 'macos-arm64'],
    ['legacy win alias', 'win-x64'],
    ['incomplete', 'linux'],
    ['extra segment', 'linux-x64-musl-extra'],
    ['path traversal injected after musl', 'linux-x64-musl-../../pwn'],
    ['uppercase', 'LINUX-x64'],
    ['leading whitespace', ' linux-x64'],
  ])('rejects invalid target: %s (%s)', async (_label, target) => {
    fs.writeFileSync(path.join(tempDir, 'entry.cjs'), 'module.exports = {}')
    await expect(
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      handler({ ...baseOpts(), entry: 'entry.cjs', target } as any, [])
    ).rejects.toMatchObject({ code: 'ERR_PNPM_PACK_APP_INVALID_TARGET' })
  })

  it.each([
    ['with forward slash', 'sub/dir'],
    ['with backslash', 'sub\\\\dir'],
    ['dot dot', '..'],
    ['relative traversal', '../pwn'],
    ['absolute', '/tmp/pwn'],
    ['dot only', '.'],
    ['null byte', 'pwn\x00'],
    ['empty', ''],
    ['Windows reserved CON', 'CON'],
    ['Windows reserved nul.exe', 'nul.exe'],
    ['Windows reserved COM1', 'COM1'],
    ['Windows colon', 'my:tool'],
    ['Windows pipe', 'my|tool'],
    ['Windows question mark', 'my?tool'],
    ['Windows asterisk', 'my*tool'],
    ['Windows lt', 'my<tool'],
    ['Windows gt', 'my>tool'],
    ['Windows quote', 'my"tool'],
    ['trailing dot', 'tool.'],
    ['trailing space', 'tool '],
  ])('rejects invalid --output-name: %s (%j)', async (_label, outputName) => {
    fs.writeFileSync(path.join(tempDir, 'entry.cjs'), 'module.exports = {}')
    await expect(
      handler(
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        { ...baseOpts(), entry: 'entry.cjs', target: 'linux-x64', outputName } as any,
        []
      )
    ).rejects.toMatchObject({ code: 'ERR_PNPM_PACK_APP_INVALID_OUTPUT_NAME' })
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
    ).rejects.toMatchObject({ code: 'ERR_PNPM_PACK_APP_MISSING_TARGET' })
  })
})
