/// <reference path="../../../../../__typings__/index.d.ts" />
import path from 'node:path'

import { licenses } from '@pnpm/deps.compliance.commands'
import { tempDir } from '@pnpm/prepare'
import { fixtures } from '@pnpm/test-fixtures'
import { readYamlFileSync } from 'read-yaml-file'

import { DEFAULT_OPTS } from './utils/index.js'

const f = fixtures(import.meta.dirname)

describe('pnpm licenses allow', () => {
  test('adds a license to the allowed list', async () => {
    const dir = tempDir()
    f.copy('simple-licenses', dir)

    const { output, exitCode } = await licenses.handler({
      ...DEFAULT_OPTS,
      dir,
      workspaceDir: dir,
      rootProjectManifestDir: dir,
      pnpmHomeDir: '',
    }, ['allow', 'MIT'])

    expect(exitCode).toBe(0)
    expect(output).toContain('Added to allowed licenses: MIT')

    const manifest = readYamlFileSync<any>(path.join(dir, 'pnpm-workspace.yaml')) // eslint-disable-line
    expect(manifest.licenses?.allowed).toStrictEqual(['MIT'])
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
    }, ['allow', 'MIT', 'Apache-2.0', 'ISC'])

    expect(exitCode).toBe(0)

    const manifest = readYamlFileSync<any>(path.join(dir, 'pnpm-workspace.yaml')) // eslint-disable-line
    expect(manifest.licenses?.allowed).toStrictEqual(['MIT', 'Apache-2.0', 'ISC'])
  })

  test('does not duplicate existing licenses', async () => {
    const dir = tempDir()
    f.copy('simple-licenses', dir)

    const { exitCode: exitCode1 } = await licenses.handler({
      ...DEFAULT_OPTS,
      dir,
      workspaceDir: dir,
      rootProjectManifestDir: dir,
      pnpmHomeDir: '',
    }, ['allow', 'MIT'])
    expect(exitCode1).toBe(0)

    const { output, exitCode: exitCode2 } = await licenses.handler({
      ...DEFAULT_OPTS,
      dir,
      workspaceDir: dir,
      rootProjectManifestDir: dir,
      pnpmHomeDir: '',
      licenses: { allowed: ['MIT'] },
    }, ['allow', 'MIT'])

    expect(exitCode2).toBe(0)
    expect(output).toContain('already in the allowed list')

    const manifest = readYamlFileSync<any>(path.join(dir, 'pnpm-workspace.yaml')) // eslint-disable-line
    expect(manifest.licenses?.allowed).toStrictEqual(['MIT'])
  })

  test('fails without arguments', async () => {
    await expect(
      licenses.handler({
        ...DEFAULT_OPTS,
        dir: tempDir(),
        workspaceDir: tempDir(),
        rootProjectManifestDir: '',
        pnpmHomeDir: '',
      }, ['allow'])
    ).rejects.toThrow('Please specify at least one license')
  })
})
