/// <reference path="../../../../../__typings__/index.d.ts" />
import fs from 'node:fs'
import path from 'node:path'

import { describe, expect, test } from '@jest/globals'
import { licenses } from '@pnpm/deps.compliance.commands'
import { prepare, tempDir } from '@pnpm/prepare'
import { fixtures } from '@pnpm/test-fixtures'
import { readYamlFileSync } from 'read-yaml-file'

import { licensesAllow } from '../../src/licenses/licensesAllow.js'
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

  test('bootstraps pnpm-workspace.yaml in a standalone project', async () => {
    prepare({ name: 'solo', version: '1.0.0' })

    const result = await licensesAllow({
      rootProjectManifestDir: process.cwd(),
      rootProjectManifest: { name: 'solo', version: '1.0.0' },
      // no workspaceDir — standalone project
      licenses: undefined,
    } as any, ['MIT']) // eslint-disable-line

    expect(result.exitCode).toBe(0)

    const ws = fs.readFileSync('pnpm-workspace.yaml', 'utf8')
    expect(ws).toContain('MIT')
  })

  test('rejects a compound (AND/OR) expression instead of storing it', async () => {
    prepare({ name: 'solo', version: '1.0.0' })

    // spdx-satisfies throws if any approved-license entry is a compound
    // (AND/OR) expression, and flattening one at input time (storing "MIT"
    // and "Apache-2.0" separately for "MIT AND Apache-2.0") would silently
    // change its meaning from "both required" to "either is enough". Reject
    // it instead of storing it.
    await expect(
      licensesAllow({
        rootProjectManifestDir: process.cwd(),
        workspaceDir: process.cwd(),
        licenses: undefined,
      } as any, ['MIT AND Apache-2.0']) // eslint-disable-line
    ).rejects.toThrow('Compound license expressions')

    expect(fs.existsSync('pnpm-workspace.yaml')).toBe(false)
  })

  test('stores a WITH exception entry verbatim', async () => {
    prepare({ name: 'solo', version: '1.0.0' })

    await licensesAllow({
      rootProjectManifestDir: process.cwd(),
      workspaceDir: process.cwd(),
      licenses: undefined,
    } as any, ['Apache-2.0 WITH LLVM-exception']) // eslint-disable-line

    const ws = fs.readFileSync('pnpm-workspace.yaml', 'utf8')
    expect(ws).toContain('Apache-2.0 WITH LLVM-exception')
  })

  test('stores a plus (or-later) entry verbatim', async () => {
    prepare({ name: 'solo', version: '1.0.0' })

    await licensesAllow({
      rootProjectManifestDir: process.cwd(),
      workspaceDir: process.cwd(),
      licenses: undefined,
    } as any, ['GPL-2.0+']) // eslint-disable-line

    const ws = fs.readFileSync('pnpm-workspace.yaml', 'utf8')
    expect(ws).toContain('GPL-2.0+')
  })

  test('allowing a license removes its case-insensitive match from the disallowed list', async () => {
    const dir = tempDir()
    f.copy('simple-licenses', dir)

    const { output, exitCode } = await licenses.handler({
      ...DEFAULT_OPTS,
      dir,
      workspaceDir: dir,
      rootProjectManifestDir: dir,
      pnpmHomeDir: '',
      licenses: { disallowed: ['MIT'] },
    }, ['allow', 'mit'])

    expect(exitCode).toBe(0)
    expect(output).toContain('Removed from disallowed licenses: MIT')

    const manifest = readYamlFileSync<any>(path.join(dir, 'pnpm-workspace.yaml')) // eslint-disable-line
    expect(manifest.licenses?.allowed).toStrictEqual(['mit'])
    expect(manifest.licenses?.disallowed ?? []).not.toContain('MIT')
  })
})
