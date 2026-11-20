import { spawnSync } from 'node:child_process'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { describe, expect, test } from '@jest/globals'

const artifactsDir = path.resolve(import.meta.dirname, '..', '..')
const verifyScript = path.join(artifactsDir, 'verify-binary.mjs')

describe('verify-binary.mjs', () => {
  test('exits 2 when target os or arch is missing', () => {
    const result = spawnSync(process.execPath, [verifyScript], {
      encoding: 'utf8',
      timeout: 10_000,
    })
    expect(result.status).toBe(2)
    expect(result.stderr).toContain('Usage: verify-binary.mjs <os> <arch> [libc]')
  })

  test('exits 1 when binary is missing in cwd', () => {
    const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'verify-binary-test-'))
    try {
      const result = spawnSync(process.execPath, [verifyScript, 'linux', 'x64'], {
        cwd: tempDir,
        encoding: 'utf8',
        timeout: 10_000,
      })
      expect(result.status).toBe(1)
      expect(result.stderr).toContain('Error: pnpm is missing')
    } finally {
      fs.rmSync(tempDir, { recursive: true, force: true })
    }
  })

  test('fails on incompatible host when target is not declared in PNPM_RELEASE_VERIFIED_TARGETS', () => {
    const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'verify-binary-test-'))
    try {
      // Pick a target that cannot match the current host
      const incompatibleOs = process.platform === 'win32' ? 'darwin' : 'win32'
      const incompatibleArch = 'arm64'
      const binName = incompatibleOs === 'win32' ? 'pnpm.exe' : 'pnpm'
      fs.writeFileSync(path.join(tempDir, binName), 'fake binary')

      const result = spawnSync(process.execPath, [verifyScript, incompatibleOs, incompatibleArch], {
        cwd: tempDir,
        encoding: 'utf8',
        timeout: 10_000,
        env: { ...process.env, PNPM_RELEASE_VERIFIED_TARGETS: '' },
      })
      expect(result.status).toBe(1)
      expect(result.stderr).toContain('Every published executable target must either execute on the current host or be covered by release verification runners')
    } finally {
      fs.rmSync(tempDir, { recursive: true, force: true })
    }
  })

  test('skips with exit 0 on incompatible host when target is declared in PNPM_RELEASE_VERIFIED_TARGETS', () => {
    const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'verify-binary-test-'))
    try {
      const incompatibleOs = process.platform === 'win32' ? 'darwin' : 'win32'
      const incompatibleArch = 'arm64'
      const binName = incompatibleOs === 'win32' ? 'pnpm.exe' : 'pnpm'
      fs.writeFileSync(path.join(tempDir, binName), 'fake binary')

      const result = spawnSync(process.execPath, [verifyScript, incompatibleOs, incompatibleArch], {
        cwd: tempDir,
        encoding: 'utf8',
        timeout: 10_000,
        env: {
          ...process.env,
          PNPM_RELEASE_VERIFIED_TARGETS: `linux-x64,${incompatibleOs}-${incompatibleArch}`,
        },
      })
      expect(result.status).toBe(0)
      expect(result.stdout).toContain(`Skipping ${binName} -v: host`)
      expect(result.stdout).toContain('covered by release verification')
    } finally {
      fs.rmSync(tempDir, { recursive: true, force: true })
    }
  })

  test('does not allow unqualified target coverage to match libc-qualified target', () => {
    const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'verify-binary-test-'))
    try {
      fs.writeFileSync(path.join(tempDir, 'pnpm'), 'fake binary')

      // If host is glibc linux, linux-x64-musl is incompatible
      const result = spawnSync(process.execPath, [verifyScript, 'linux', 'x64', 'musl'], {
        cwd: tempDir,
        encoding: 'utf8',
        timeout: 10_000,
        env: {
          ...process.env,
          PNPM_RELEASE_VERIFIED_TARGETS: 'linux-x64',
        },
      })
      if (process.platform === 'linux') {
        const header = process.report.getReport().header
        const isGlibc = Boolean(header.glibcVersionRuntime)
        if (isGlibc) {
          expect(result.status).toBe(1)
          expect(result.stderr).toContain('not declared in PNPM_RELEASE_VERIFIED_TARGETS')
        }
      } else {
        expect(result.status).toBe(1)
        expect(result.stderr).toContain('not declared in PNPM_RELEASE_VERIFIED_TARGETS')
      }
    } finally {
      fs.rmSync(tempDir, { recursive: true, force: true })
    }
  })
})
