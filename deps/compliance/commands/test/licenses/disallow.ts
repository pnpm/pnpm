/// <reference path="../../../../../__typings__/index.d.ts" />
import path from 'node:path'

import { licenses } from '@pnpm/deps.compliance.commands'
import { tempDir } from '@pnpm/prepare'
import { fixtures } from '@pnpm/test-fixtures'
import { readYamlFileSync } from 'read-yaml-file'

import { DEFAULT_OPTS } from './utils/index.js'

const f = fixtures(import.meta.dirname)

describe('pnpm licenses disallow', () => {
  test('adds a license to the disallowed list', async () => {
    const dir = tempDir()
    f.copy('simple-licenses', dir)

    const { output, exitCode } = await licenses.handler({
      ...DEFAULT_OPTS,
      dir,
      workspaceDir: dir,
      rootProjectManifestDir: dir,
      pnpmHomeDir: '',
    }, ['disallow', 'GPL-3.0-only'])

    expect(exitCode).toBe(0)
    expect(output).toContain('Added to disallowed licenses: GPL-3.0-only')

    const manifest = readYamlFileSync<any>(path.join(dir, 'pnpm-workspace.yaml')) // eslint-disable-line
    expect(manifest.licenses?.disallowed).toStrictEqual(['GPL-3.0-only'])
  })

  test('adds multiple licenses at once', async () => {
    const dir = tempDir()
    f.copy('simple-licenses', dir)

    const { exitCode } = await licenses.handler({
      ...DEFAULT_OPTS,
      dir,
      workspaceDir: dir,
      rootProjectManifestDir: dir,
      pnpmHomeDir: '',
    }, ['disallow', 'GPL-3.0-only', 'AGPL-3.0-only'])

    expect(exitCode).toBe(0)

    const manifest = readYamlFileSync<any>(path.join(dir, 'pnpm-workspace.yaml')) // eslint-disable-line
    expect(manifest.licenses?.disallowed).toStrictEqual(['GPL-3.0-only', 'AGPL-3.0-only'])
  })

  test('does not duplicate existing licenses', async () => {
    const dir = tempDir()
    f.copy('simple-licenses', dir)

    await licenses.handler({
      ...DEFAULT_OPTS,
      dir,
      workspaceDir: dir,
      rootProjectManifestDir: dir,
      pnpmHomeDir: '',
    }, ['disallow', 'GPL-3.0-only'])

    const { output } = await licenses.handler({
      ...DEFAULT_OPTS,
      dir,
      workspaceDir: dir,
      rootProjectManifestDir: dir,
      pnpmHomeDir: '',
      licenses: { disallowed: ['GPL-3.0-only'] },
    }, ['disallow', 'GPL-3.0-only'])

    expect(output).toContain('already in the disallowed list')

    const manifest = readYamlFileSync<any>(path.join(dir, 'pnpm-workspace.yaml')) // eslint-disable-line
    expect(manifest.licenses?.disallowed).toStrictEqual(['GPL-3.0-only'])
  })

  test('fails without arguments', async () => {
    await expect(
      licenses.handler({
        ...DEFAULT_OPTS,
        dir: tempDir(),
        workspaceDir: tempDir(),
        rootProjectManifestDir: '',
        pnpmHomeDir: '',
      }, ['disallow'])
    ).rejects.toThrow('Please specify at least one license')
  })
})
