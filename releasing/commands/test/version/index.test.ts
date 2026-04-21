import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { afterEach, beforeEach, describe, expect, it } from '@jest/globals'
import { safeExeca as execa } from 'execa'

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
    expect(types['git-checks']).toBe(Boolean)
    expect(types['commit-hooks']).toBe(Boolean)
    expect(types['git-tag-version']).toBe(Boolean)
    expect(types['sign-git-tag']).toBe(Boolean)
    expect(types.recursive).toBe(Boolean)
  })

  it('should throw error with invalid bump type', async () => {
    fs.writeFileSync(path.join(tempDir, 'package.json'), JSON.stringify({ name: 'test-pkg', version: '1.0.0' }))

    await expect(
      handler({
        dir: tempDir,
        workspaceDir: tempDir,
        gitChecks: false,
        gitTagVersion: false,
      } as any, ['invalid']) // eslint-disable-line @typescript-eslint/no-explicit-any
    ).rejects.toMatchObject({ code: 'ERR_PNPM_INVALID_VERSION_BUMP' })
  })

  it('should throw error when no bump type is provided', async () => {
    fs.writeFileSync(path.join(tempDir, 'package.json'), JSON.stringify({ name: 'test-pkg', version: '1.0.0' }))

    await expect(
      handler({
        dir: tempDir,
        workspaceDir: tempDir,
        gitChecks: false,
        gitTagVersion: false,
      } as any, []) // eslint-disable-line @typescript-eslint/no-explicit-any
    ).rejects.toMatchObject({ code: 'ERR_PNPM_INVALID_VERSION_BUMP' })
  })

  it('should bump major version', async () => {
    fs.writeFileSync(path.join(tempDir, 'package.json'), JSON.stringify({ name: 'test-pkg', version: '1.2.3' }))

    const result = await handler({
      dir: tempDir,
      workspaceDir: tempDir,
      gitChecks: false,
      gitTagVersion: false,
    } as any, ['major']) // eslint-disable-line @typescript-eslint/no-explicit-any

    expect(result).toContain('1.2.3 → 2.0.0')
    const updated = JSON.parse(fs.readFileSync(path.join(tempDir, 'package.json'), 'utf-8'))
    expect(updated.version).toBe('2.0.0')
  })

  it('should bump minor version', async () => {
    fs.writeFileSync(path.join(tempDir, 'package.json'), JSON.stringify({ name: 'test-pkg', version: '1.0.0' }))

    const result = await handler({
      dir: tempDir,
      workspaceDir: tempDir,
      gitChecks: false,
      gitTagVersion: false,
    } as any, ['minor']) // eslint-disable-line @typescript-eslint/no-explicit-any

    expect(result).toContain('1.0.0 → 1.1.0')
    const updated = JSON.parse(fs.readFileSync(path.join(tempDir, 'package.json'), 'utf-8'))
    expect(updated.version).toBe('1.1.0')
  })

  it('should bump patch version', async () => {
    fs.writeFileSync(path.join(tempDir, 'package.json'), JSON.stringify({ name: 'test-pkg', version: '1.0.0' }))

    const result = await handler({
      dir: tempDir,
      workspaceDir: tempDir,
      gitChecks: false,
      gitTagVersion: false,
    } as any, ['patch']) // eslint-disable-line @typescript-eslint/no-explicit-any

    expect(result).toContain('1.0.0 → 1.0.1')
    const updated = JSON.parse(fs.readFileSync(path.join(tempDir, 'package.json'), 'utf-8'))
    expect(updated.version).toBe('1.0.1')
  })

  it('should support prerelease versions', async () => {
    fs.writeFileSync(path.join(tempDir, 'package.json'), JSON.stringify({ name: 'test-pkg', version: '1.0.0' }))

    const result = await handler({
      dir: tempDir,
      workspaceDir: tempDir,
      gitChecks: false,
      gitTagVersion: false,
      preid: 'alpha',
    } as any, ['prerelease']) // eslint-disable-line @typescript-eslint/no-explicit-any

    expect(result).toContain('1.0.0 → 1.0.1-alpha.0')
  })

  it('should return JSON output when --json is set', async () => {
    fs.writeFileSync(path.join(tempDir, 'package.json'), JSON.stringify({ name: 'test-pkg', version: '1.0.0' }))

    const result = await handler({
      dir: tempDir,
      workspaceDir: tempDir,
      gitChecks: false,
      gitTagVersion: false,
      json: true,
    } as any, ['patch']) // eslint-disable-line @typescript-eslint/no-explicit-any

    const parsed = JSON.parse(result as string)
    expect(parsed).toEqual([
      expect.objectContaining({
        name: 'test-pkg',
        currentVersion: '1.0.0',
        newVersion: '1.0.1',
      }),
    ])
  })

  it('should set an explicit version when a valid semver version is passed', async () => {
    fs.writeFileSync(path.join(tempDir, 'package.json'), JSON.stringify({ name: 'test-pkg', version: '1.2.3' }))

    const result = await handler({
      dir: tempDir,
      workspaceDir: tempDir,
      gitChecks: false,
      gitTagVersion: false,
    } as any, ['0.0.0']) // eslint-disable-line @typescript-eslint/no-explicit-any

    expect(result).toContain('1.2.3 → 0.0.0')
    const updated = JSON.parse(fs.readFileSync(path.join(tempDir, 'package.json'), 'utf-8'))
    expect(updated.version).toBe('0.0.0')
  })

  it('should set an explicit prerelease version when passed', async () => {
    fs.writeFileSync(path.join(tempDir, 'package.json'), JSON.stringify({ name: 'test-pkg', version: '1.0.0' }))

    const result = await handler({
      dir: tempDir,
      workspaceDir: tempDir,
      gitChecks: false,
      gitTagVersion: false,
    } as any, ['2.0.0-beta.1']) // eslint-disable-line @typescript-eslint/no-explicit-any

    expect(result).toContain('1.0.0 → 2.0.0-beta.1')
  })

  it('should throw error with invalid semver version', async () => {
    fs.writeFileSync(path.join(tempDir, 'package.json'), JSON.stringify({ name: 'test-pkg', version: '1.0.0' }))

    await expect(
      handler({
        dir: tempDir,
        workspaceDir: tempDir,
        gitChecks: false,
        gitTagVersion: false,
      } as any, ['not-a-version']) // eslint-disable-line @typescript-eslint/no-explicit-any
    ).rejects.toMatchObject({ code: 'ERR_PNPM_INVALID_VERSION_BUMP' })
  })

  it('should allow bumping with --allow-same-version', async () => {
    fs.writeFileSync(path.join(tempDir, 'package.json'), JSON.stringify({ name: 'test-pkg', version: '1.0.0' }))

    const result = await handler({
      dir: tempDir,
      workspaceDir: tempDir,
      gitChecks: false,
      gitTagVersion: false,
      allowSameVersion: true,
    } as any, ['patch']) // eslint-disable-line @typescript-eslint/no-explicit-any

    expect(result).toContain('1.0.0 → 1.0.1')
  })

  it('should throw when package has no name or version', async () => {
    fs.writeFileSync(path.join(tempDir, 'package.json'), JSON.stringify({}))

    await expect(
      handler({
        dir: tempDir,
        workspaceDir: tempDir,
        gitChecks: false,
        gitTagVersion: false,
      } as any, ['patch']) // eslint-disable-line @typescript-eslint/no-explicit-any
    ).rejects.toMatchObject({ code: 'ERR_PNPM_NO_PACKAGES_TO_VERSION' })
  })

  it('should throw when package has an invalid version', async () => {
    fs.writeFileSync(path.join(tempDir, 'package.json'), JSON.stringify({ name: 'test-pkg', version: 'not-a-version' }))

    await expect(
      handler({
        dir: tempDir,
        workspaceDir: tempDir,
        gitChecks: false,
        gitTagVersion: false,
      } as any, ['patch']) // eslint-disable-line @typescript-eslint/no-explicit-any
    ).rejects.toMatchObject({ code: 'ERR_PNPM_INVALID_VERSION' })
  })

  describe('git integration', () => {
    let origCwd: string

    beforeEach(async () => {
      origCwd = process.cwd()
      process.chdir(tempDir)
      await execa('git', ['init', '-q'], { cwd: tempDir })
      await execa('git', ['config', 'user.email', 'x@y.z'], { cwd: tempDir })
      await execa('git', ['config', 'user.name', 'xyz'], { cwd: tempDir })
      await execa('git', ['config', 'commit.gpgSign', 'false'], { cwd: tempDir })
      await execa('git', ['config', 'tag.gpgSign', 'false'], { cwd: tempDir })
      fs.writeFileSync(path.join(tempDir, 'package.json'), JSON.stringify({ name: 'test-pkg', version: '1.0.0' }))
      await execa('git', ['add', '.'], { cwd: tempDir })
      await execa('git', ['commit', '-q', '-m', 'init', '--no-gpg-sign'], { cwd: tempDir })
    })

    afterEach(() => {
      process.chdir(origCwd)
    })

    it('should create a git commit and tag by default', async () => {
      const result = await handler({
        dir: tempDir,
        workspaceDir: tempDir,
      } as any, ['patch']) // eslint-disable-line @typescript-eslint/no-explicit-any

      expect(result).toContain('1.0.0 → 1.0.1')

      const { stdout: tags } = await execa('git', ['tag', '--list'], { cwd: tempDir })
      expect(tags).toBe('v1.0.1')

      const { stdout: logSubject } = await execa('git', ['log', '-1', '--pretty=%s'], { cwd: tempDir })
      expect(logSubject).toBe('1.0.1')
    })

    it('should use a custom tag prefix when tagVersionPrefix is set', async () => {
      await handler({
        dir: tempDir,
        workspaceDir: tempDir,
        tagVersionPrefix: 'release-',
      } as any, ['patch']) // eslint-disable-line @typescript-eslint/no-explicit-any

      const { stdout: tags } = await execa('git', ['tag', '--list'], { cwd: tempDir })
      expect(tags).toBe('release-1.0.1')
    })

    it('should substitute %s in the commit message', async () => {
      await handler({
        dir: tempDir,
        workspaceDir: tempDir,
        message: 'chore: release %s',
      } as any, ['patch']) // eslint-disable-line @typescript-eslint/no-explicit-any

      const { stdout: logSubject } = await execa('git', ['log', '-1', '--pretty=%s'], { cwd: tempDir })
      expect(logSubject).toBe('chore: release 1.0.1')
    })

    it('should not create a commit or tag with --no-git-tag-version', async () => {
      const { stdout: commitsBefore } = await execa('git', ['rev-list', '--count', 'HEAD'], { cwd: tempDir })

      await handler({
        dir: tempDir,
        workspaceDir: tempDir,
        gitTagVersion: false,
      } as any, ['0.0.0']) // eslint-disable-line @typescript-eslint/no-explicit-any

      const { stdout: tags } = await execa('git', ['tag', '--list'], { cwd: tempDir })
      expect(tags).toBe('')

      const { stdout: commitsAfter } = await execa('git', ['rev-list', '--count', 'HEAD'], { cwd: tempDir })
      expect(commitsAfter).toBe(commitsBefore)
    })

    it('should tag --allow-same-version runs even when the manifest is unchanged', async () => {
      await handler({
        dir: tempDir,
        workspaceDir: tempDir,
        allowSameVersion: true,
      } as any, ['1.0.0']) // eslint-disable-line @typescript-eslint/no-explicit-any

      const { stdout: tags } = await execa('git', ['tag', '--list'], { cwd: tempDir })
      expect(tags).toBe('v1.0.0')

      const { stdout: logSubject } = await execa('git', ['log', '-1', '--pretty=%s'], { cwd: tempDir })
      expect(logSubject).toBe('1.0.0')
    })

    it('should bypass commit hooks with --no-commit-hooks', async () => {
      const hookDir = path.join(tempDir, '.git', 'hooks')
      const hookPath = path.join(hookDir, 'pre-commit')
      fs.writeFileSync(hookPath, '#!/bin/sh\nexit 1\n')
      fs.chmodSync(hookPath, 0o755)

      await handler({
        dir: tempDir,
        workspaceDir: tempDir,
        commitHooks: false,
      } as any, ['patch']) // eslint-disable-line @typescript-eslint/no-explicit-any

      const { stdout: tags } = await execa('git', ['tag', '--list'], { cwd: tempDir })
      expect(tags).toBe('v1.0.1')
    })

    it('should skip commit and tag in recursive mode', async () => {
      const pkgADir = path.join(tempDir, 'packages', 'pkg-a')
      const pkgBDir = path.join(tempDir, 'packages', 'pkg-b')
      fs.mkdirSync(pkgADir, { recursive: true })
      fs.mkdirSync(pkgBDir, { recursive: true })

      fs.writeFileSync(path.join(tempDir, 'pnpm-workspace.yaml'), 'packages:\n  - "packages/*"\n')
      fs.writeFileSync(path.join(pkgADir, 'package.json'), JSON.stringify({ name: 'pkg-a', version: '1.0.0' }))
      fs.writeFileSync(path.join(pkgBDir, 'package.json'), JSON.stringify({ name: 'pkg-b', version: '2.3.0' }))

      await execa('git', ['add', '.'], { cwd: tempDir })
      await execa('git', ['commit', '-q', '-m', 'add workspace', '--no-gpg-sign'], { cwd: tempDir })

      const { stdout: commitsBefore } = await execa('git', ['rev-list', '--count', 'HEAD'], { cwd: tempDir })

      await handler({
        dir: tempDir,
        workspaceDir: tempDir,
        recursive: true,
      } as any, ['patch']) // eslint-disable-line @typescript-eslint/no-explicit-any

      const { stdout: tags } = await execa('git', ['tag', '--list'], { cwd: tempDir })
      expect(tags).toBe('')

      const { stdout: commitsAfter } = await execa('git', ['rev-list', '--count', 'HEAD'], { cwd: tempDir })
      expect(commitsAfter).toBe(commitsBefore)
    })
  })

  describe('recursive mode', () => {
    it('should bump versions of all workspace packages with --recursive', async () => {
      // Create workspace structure
      const pkgADir = path.join(tempDir, 'packages', 'pkg-a')
      const pkgBDir = path.join(tempDir, 'packages', 'pkg-b')
      fs.mkdirSync(pkgADir, { recursive: true })
      fs.mkdirSync(pkgBDir, { recursive: true })

      fs.writeFileSync(path.join(tempDir, 'package.json'), JSON.stringify({ name: 'my-workspace', version: '1.0.0' }))
      fs.writeFileSync(path.join(tempDir, 'pnpm-workspace.yaml'), 'packages:\n  - "packages/*"\n')
      fs.writeFileSync(path.join(pkgADir, 'package.json'), JSON.stringify({ name: 'pkg-a', version: '1.0.0' }))
      fs.writeFileSync(path.join(pkgBDir, 'package.json'), JSON.stringify({ name: 'pkg-b', version: '2.3.0' }))

      const result = await handler({
        dir: tempDir,
        workspaceDir: tempDir,
        gitChecks: false,
        gitTagVersion: false,
        recursive: true,
      } as any, ['minor']) // eslint-disable-line @typescript-eslint/no-explicit-any

      const resultStr = result as string
      expect(resultStr).toContain('pkg-a')
      expect(resultStr).toContain('pkg-b')

      const manifestA = JSON.parse(fs.readFileSync(path.join(pkgADir, 'package.json'), 'utf-8'))
      const manifestB = JSON.parse(fs.readFileSync(path.join(pkgBDir, 'package.json'), 'utf-8'))
      expect(manifestA.version).toBe('1.1.0')
      expect(manifestB.version).toBe('2.4.0')
    })

    it('should return JSON output in recursive mode with --json', async () => {
      const pkgDir = path.join(tempDir, 'packages', 'pkg-a')
      fs.mkdirSync(pkgDir, { recursive: true })

      fs.writeFileSync(path.join(tempDir, 'package.json'), JSON.stringify({ name: 'my-workspace', version: '1.0.0' }))
      fs.writeFileSync(path.join(tempDir, 'pnpm-workspace.yaml'), 'packages:\n  - "packages/*"\n')
      fs.writeFileSync(path.join(pkgDir, 'package.json'), JSON.stringify({ name: 'pkg-a', version: '1.0.0' }))

      const result = await handler({
        dir: tempDir,
        workspaceDir: tempDir,
        gitChecks: false,
        gitTagVersion: false,
        recursive: true,
        json: true,
      } as any, ['patch']) // eslint-disable-line @typescript-eslint/no-explicit-any

      const parsed = JSON.parse(result as string)
      expect(parsed).toEqual(
        expect.arrayContaining([
          expect.objectContaining({
            name: 'pkg-a',
            currentVersion: '1.0.0',
            newVersion: '1.0.1',
          }),
        ])
      )
    })

    it('should skip workspace packages without name or version', async () => {
      const pkgADir = path.join(tempDir, 'packages', 'pkg-a')
      const pkgBDir = path.join(tempDir, 'packages', 'pkg-b')
      fs.mkdirSync(pkgADir, { recursive: true })
      fs.mkdirSync(pkgBDir, { recursive: true })

      fs.writeFileSync(path.join(tempDir, 'package.json'), JSON.stringify({ name: 'my-workspace', version: '1.0.0' }))
      fs.writeFileSync(path.join(tempDir, 'pnpm-workspace.yaml'), 'packages:\n  - "packages/*"\n')
      fs.writeFileSync(path.join(pkgADir, 'package.json'), JSON.stringify({ name: 'pkg-a', version: '1.0.0' }))
      fs.writeFileSync(path.join(pkgBDir, 'package.json'), JSON.stringify({ private: true }))

      const result = await handler({
        dir: tempDir,
        workspaceDir: tempDir,
        gitChecks: false,
        gitTagVersion: false,
        recursive: true,
      } as any, ['patch']) // eslint-disable-line @typescript-eslint/no-explicit-any

      const resultStr = result as string
      expect(resultStr).toContain('pkg-a')
      expect(resultStr).not.toContain('pkg-b')
    })

    it('should not enter recursive mode without --recursive flag', async () => {
      // Create workspace structure but don't pass --recursive
      const pkgDir = path.join(tempDir, 'packages', 'pkg-a')
      fs.mkdirSync(pkgDir, { recursive: true })

      fs.writeFileSync(path.join(tempDir, 'package.json'), JSON.stringify({ name: 'my-workspace', version: '1.0.0' }))
      fs.writeFileSync(path.join(tempDir, 'pnpm-workspace.yaml'), 'packages:\n  - "packages/*"\n')
      fs.writeFileSync(path.join(pkgDir, 'package.json'), JSON.stringify({ name: 'pkg-a', version: '1.0.0' }))

      // Without --recursive, should only bump the root package
      const result = await handler({
        dir: tempDir,
        workspaceDir: tempDir,
        gitChecks: false,
        gitTagVersion: false,
      } as any, ['patch']) // eslint-disable-line @typescript-eslint/no-explicit-any

      const resultStr = result as string
      expect(resultStr).toContain('my-workspace')
      expect(resultStr).not.toContain('pkg-a')

      // Root was bumped
      const rootManifest = JSON.parse(fs.readFileSync(path.join(tempDir, 'package.json'), 'utf-8'))
      expect(rootManifest.version).toBe('1.0.1')

      // Sub-package was NOT bumped
      const pkgManifest = JSON.parse(fs.readFileSync(path.join(pkgDir, 'package.json'), 'utf-8'))
      expect(pkgManifest.version).toBe('1.0.0')
    })
  })
})
