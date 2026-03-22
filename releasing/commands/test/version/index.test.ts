import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { afterEach, beforeEach, describe, expect, it } from '@jest/globals'

import { version } from '../../src/index.js'

describe('version command', () => {
  const { cliOptionsTypes, commandNames, handler, help } = version
  let tempDir: string

  beforeEach(() => {
    tempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-version-test-'))
  })

  afterEach(() => {
    if (fs.existsSync(tempDir)) {
      fs.rmSync(tempDir, { recursive: true })
    }
  })

  it('should export correct command names', () => {
    expect(commandNames).toEqual(['version'])
  })

  it('should provide help text', () => {
    const helpText = help()
    expect(helpText).toContain('Bumps the version')
    expect(helpText).toContain('major')
    expect(helpText).toContain('minor')
  })

  it('should have correct cli option types', () => {
    const types = cliOptionsTypes()
    expect(types['allow-same-version']).toBe(Boolean)
    expect(types['no-git-checks']).toBe(Boolean)
    expect(types.recursive).toBe(Boolean)
  })

  it('should throw error with invalid bump type', async () => {
    expect.assertions(1)
    const manifest = {
      name: 'test-pkg',
      version: '1.0.0',
    }
    fs.writeFileSync(path.join(tempDir, 'package.json'), JSON.stringify(manifest, null, 2))

    try {
      await handler({
        dir: tempDir,
        workspaceDir: tempDir,
        workspaceRoot: tempDir,
        noGitChecks: true,
      } as any, ['invalid']) // eslint-disable-line @typescript-eslint/no-explicit-any
    } catch (err: unknown) {
      expect((err as { code: string }).code).toBe('ERR_PNPM_INVALID_VERSION_BUMP')
    }
  })

  it('should bump version correctly', async () => {
    const manifest = {
      name: 'test-pkg',
      version: '1.0.0',
    }
    fs.writeFileSync(path.join(tempDir, 'package.json'), JSON.stringify(manifest, null, 2))

    const result = await handler({
      dir: tempDir,
      workspaceDir: tempDir,
      workspaceRoot: tempDir,
      noGitChecks: true,
    } as any, ['minor']) // eslint-disable-line @typescript-eslint/no-explicit-any

    expect(result).toContain('1.0.0 → 1.1.0')

    const updatedManifest = JSON.parse(
      fs.readFileSync(path.join(tempDir, 'package.json'), 'utf-8')
    )
    expect(updatedManifest.version).toBe('1.1.0')
  })

  it('should support prerelease versions', async () => {
    const manifest = {
      name: 'test-pkg',
      version: '1.0.0',
    }
    fs.writeFileSync(path.join(tempDir, 'package.json'), JSON.stringify(manifest, null, 2))

    const result = await handler({
      dir: tempDir,
      workspaceDir: tempDir,
      workspaceRoot: tempDir,
      noGitChecks: true,
      preid: 'alpha',
    } as any, ['prerelease']) // eslint-disable-line @typescript-eslint/no-explicit-any

    expect(result).toContain('1.0.0 → 1.0.1-alpha.0')
  })
})
