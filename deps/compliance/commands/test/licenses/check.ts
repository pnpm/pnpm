/// <reference path="../../../../../__typings__/index.d.ts" />
import path from 'node:path'
import { stripVTControlCharacters as stripAnsi } from 'node:util'

import { STORE_VERSION } from '@pnpm/constants'
import { licenses } from '@pnpm/deps.compliance.commands'
import { install } from '@pnpm/installing.commands'
import { tempDir } from '@pnpm/prepare'
import { fixtures } from '@pnpm/test-fixtures'
import { filterProjectsBySelectorObjectsFromDir } from '@pnpm/workspace.projects-filter'

import { DEFAULT_OPTS } from './utils/index.js'

const f = fixtures(import.meta.dirname)

async function setupProject (fixtureName: string): Promise<{ dir: string, storeDir: string }> {
  const dir = tempDir()
  f.copy(fixtureName, dir)

  const storeDir = path.join(dir, 'store')
  await install.handler({
    ...DEFAULT_OPTS,
    dir,
    pnpmHomeDir: '',
    storeDir,
  })

  return { dir, storeDir: path.resolve(storeDir, STORE_VERSION) }
}

describe('pnpm licenses check', () => {
  test('passes with no licenses config', async () => {
    const { dir, storeDir } = await setupProject('simple-licenses')

    const { output, exitCode } = await licenses.handler({
      ...DEFAULT_OPTS,
      dir,
      pnpmHomeDir: '',
      storeDir,
    }, ['check'])

    expect(exitCode).toBe(0)
    expect(output).toContain('passed the license check')
  })

  test('passes when all licenses are in the allowed list', async () => {
    const { dir, storeDir } = await setupProject('simple-licenses')

    const { output, exitCode } = await licenses.handler({
      ...DEFAULT_OPTS,
      dir,
      pnpmHomeDir: '',
      storeDir,
      licenses: {
        allowed: ['MIT'],
        mode: 'strict',
      },
    }, ['check'])

    expect(exitCode).toBe(0)
    expect(output).toContain('passed the license check')
  })

  test('fails when a license is not in the allowed list (strict mode)', async () => {
    const { dir, storeDir } = await setupProject('simple-licenses')

    const { output, exitCode } = await licenses.handler({
      ...DEFAULT_OPTS,
      dir,
      pnpmHomeDir: '',
      storeDir,
      licenses: {
        allowed: ['Apache-2.0'],
        mode: 'strict',
      },
    }, ['check'])

    expect(exitCode).toBe(1)
    expect(stripAnsi(output)).toContain('is-positive')
    expect(stripAnsi(output)).toContain('not in the allowed list')
  })

  test('fails when a license is in the disallowed list', async () => {
    const { dir, storeDir } = await setupProject('simple-licenses')

    const { output, exitCode } = await licenses.handler({
      ...DEFAULT_OPTS,
      dir,
      pnpmHomeDir: '',
      storeDir,
      licenses: {
        disallowed: ['MIT'],
        mode: 'strict',
      },
    }, ['check'])

    expect(exitCode).toBe(1)
    expect(stripAnsi(output)).toContain('is-positive')
    expect(stripAnsi(output)).toContain('disallowed')
  })

  test('disallowed licenses fail even in loose mode', async () => {
    const { dir, storeDir } = await setupProject('simple-licenses')

    const { output, exitCode } = await licenses.handler({
      ...DEFAULT_OPTS,
      dir,
      pnpmHomeDir: '',
      storeDir,
      licenses: {
        disallowed: ['MIT'],
        mode: 'loose',
      },
    }, ['check'])

    expect(exitCode).toBe(1)
    expect(stripAnsi(output)).toContain('violation')
  })

  test('unlisted licenses pass in loose mode', async () => {
    const { dir, storeDir } = await setupProject('simple-licenses')

    const { exitCode } = await licenses.handler({
      ...DEFAULT_OPTS,
      dir,
      pnpmHomeDir: '',
      storeDir,
      licenses: {
        allowed: ['Apache-2.0'],
        mode: 'loose',
      },
    }, ['check'])

    // MIT is not in the allowed list, but loose mode does not reject unlisted
    expect(exitCode).toBe(0)
  })

  test('override allows a specific package', async () => {
    const { dir, storeDir } = await setupProject('simple-licenses')

    const { exitCode } = await licenses.handler({
      ...DEFAULT_OPTS,
      dir,
      pnpmHomeDir: '',
      storeDir,
      licenses: {
        allowed: ['Apache-2.0'],
        mode: 'strict',
        overrides: { 'is-positive': true },
      },
    }, ['check'])

    expect(exitCode).toBe(0)
  })

  test('outputs JSON when --json is passed', async () => {
    const { dir, storeDir } = await setupProject('simple-licenses')

    const { output, exitCode } = await licenses.handler({
      ...DEFAULT_OPTS,
      dir,
      pnpmHomeDir: '',
      json: true,
      storeDir,
      licenses: {
        disallowed: ['MIT'],
        mode: 'strict',
      },
    }, ['check'])

    expect(exitCode).toBe(1)
    const parsed = JSON.parse(output)
    expect(parsed.violations).toHaveLength(1)
    expect(parsed.violations[0].packageName).toBe('is-positive')
    expect(parsed.checkedCount).toBeGreaterThan(0)
  })

  test('environment parameter overrides config', async () => {
    // with-dev-dependency has is-positive (MIT) in dependencies
    // and typescript (Apache-2.0) in devDependencies
    const { dir, storeDir } = await setupProject('with-dev-dependency')

    // Disallow MIT, but check only dev environment via positional param
    // The "dev" param should override the config's environment
    const { exitCode } = await licenses.handler({
      ...DEFAULT_OPTS,
      dir,
      pnpmHomeDir: '',
      storeDir,
      licenses: {
        disallowed: ['MIT'],
        mode: 'strict',
        environment: 'prod',
      },
    }, ['check', 'dev'])

    // MIT is only in prod deps (is-positive), not dev deps (typescript is Apache-2.0)
    // Since we override to "dev", MIT disallow should not trigger
    expect(exitCode).toBe(0)
  })

  test('rejects invalid environment parameter', async () => {
    const { dir, storeDir } = await setupProject('simple-licenses')

    await expect(
      licenses.handler({
        ...DEFAULT_OPTS,
        dir,
        pnpmHomeDir: '',
        storeDir,
      }, ['check', 'invalid'])
    ).rejects.toThrow('Unknown environment')
  })

  test('depth shallow with workspace selectedProjectsGraph', async () => {
    // workspace-licenses has:
    //   bar/ depends on is-positive (MIT) — direct dep
    //   foo/ depends on react, react-dom (MIT) — direct deps
    // Their transitive deps should be excluded by shallow mode
    const workspaceDir = tempDir()
    f.copy('workspace-licenses', workspaceDir)

    const { allProjects, allProjectsGraph, selectedProjectsGraph } =
      await filterProjectsBySelectorObjectsFromDir(workspaceDir, [])

    const storeDir = path.join(workspaceDir, 'store')
    await install.handler({
      ...DEFAULT_OPTS,
      dir: workspaceDir,
      workspaceDir,
      lockfileDir: workspaceDir,
      pnpmHomeDir: '',
      storeDir,
      allProjects,
      allProjectsGraph,
      selectedProjectsGraph,
    })

    // First, run check without shallow to get total count
    const { output: deepOutput } = await licenses.handler({
      ...DEFAULT_OPTS,
      dir: workspaceDir,
      lockfileDir: workspaceDir,
      pnpmHomeDir: '',
      json: true,
      storeDir: path.resolve(storeDir, STORE_VERSION),
      selectedProjectsGraph,
      licenses: {
        mode: 'loose',
      },
    }, ['check'])

    const deepResult = JSON.parse(deepOutput)

    // Now run with shallow — should check fewer packages
    const { output: shallowOutput, exitCode } = await licenses.handler({
      ...DEFAULT_OPTS,
      dir: workspaceDir,
      lockfileDir: workspaceDir,
      pnpmHomeDir: '',
      json: true,
      storeDir: path.resolve(storeDir, STORE_VERSION),
      selectedProjectsGraph,
      licenses: {
        mode: 'loose',
        depth: 'shallow',
      },
    }, ['check'])

    expect(exitCode).toBe(0)
    const shallowResult = JSON.parse(shallowOutput)

    // Shallow should check fewer or equal packages than deep
    expect(shallowResult.checkedCount).toBeLessThanOrEqual(deepResult.checkedCount)
    // Shallow should still check the direct deps from both workspace projects
    expect(shallowResult.checkedCount).toBeGreaterThan(0)
  })
})
