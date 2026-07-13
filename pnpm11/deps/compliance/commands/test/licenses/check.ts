/// <reference path="../../../../../__typings__/index.d.ts" />
import path from 'node:path'
import { stripVTControlCharacters as stripAnsi } from 'node:util'

import { describe, expect, test } from '@jest/globals'
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
    // Fixtures under the shared `__fixtures__` dir are installed by CI's
    // `prepare-fixtures` step, so `f.copy` brings their `node_modules` (and its
    // `.modules.yaml`, which records a different store) into the temp dir. The
    // install then wants to purge that modules dir, and the purge prompts for
    // confirmation — which aborts on a machine with no TTY. Tests always want
    // the purge, so skip the prompt.
    confirmModulesPurge: false,
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

    // No allowed/disallowed/overrides configured means there is no policy to
    // check against, so the command short-circuits without scanning.
    expect(exitCode).toBe(0)
    expect(output).toContain('No license policy configured')
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

  test('rejects extra positional arguments', async () => {
    const { dir, storeDir } = await setupProject('simple-licenses')

    await expect(
      licenses.handler({
        ...DEFAULT_OPTS,
        dir,
        pnpmHomeDir: '',
        storeDir,
      }, ['check', 'prod', 'extra'])
    ).rejects.toThrow('Too many arguments')
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

    // A policy (allowed list) must be configured for `licenses check` to scan
    // at all; loose mode downgrades any non-MIT license to a warning rather
    // than a violation, so this doesn't affect checkedCount or exitCode.
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
        allowed: ['MIT'],
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
        allowed: ['MIT'],
      },
    }, ['check'])

    expect(exitCode).toBe(0)
    const shallowResult = JSON.parse(shallowOutput)

    // Shallow should check fewer or equal packages than deep
    expect(shallowResult.checkedCount).toBeLessThanOrEqual(deepResult.checkedCount)
    // Shallow should still check the direct deps from both workspace projects
    expect(shallowResult.checkedCount).toBeGreaterThan(0)
  })

  // Regression test for #4: shallow mode used to derive direct deps from the
  // manifest's dependency KEYS ('positive', the alias), which never matched
  // the scanner-reported package name ('is-positive'), so aliased direct
  // deps silently escaped the shallow filter. `collectDirectDepKeys` now
  // resolves `npm:` aliases through the lockfile to the real package name,
  // so the aliased dep is evaluated under its actual license.
  // A hand-edited pnpm-workspace.yaml can put a compound (AND/OR) SPDX
  // expression directly into `licenses.disallowed`, bypassing the rejection
  // `pnpm licenses allow/disallow` applies at input time. Without a scan-time
  // guard this is a silent fail-open: the matcher only compares single leaf
  // candidates against the disallowed set, so the compound never matches any
  // one leaf and nothing gets blocked. `scanAndCheckLicenses` now rejects it
  // up front, so `pnpm licenses check` throws instead of silently passing.
  test('rejects a hand-edited compound expression in the disallowed list', async () => {
    const { dir, storeDir } = await setupProject('simple-licenses')

    await expect(
      licenses.handler({
        ...DEFAULT_OPTS,
        dir,
        pnpmHomeDir: '',
        storeDir,
        licenses: {
          disallowed: ['GPL-3.0-only OR GPL-2.0-only'],
          mode: 'strict',
        },
      }, ['check'])
    ).rejects.toThrow('Compound license expressions')
  })

  test('depth shallow evaluates an aliased dependency under its real package name (regression #4)', async () => {
    // with-aliased-dep declares `dependencies: { positive: "npm:is-positive@1.0.0" }`
    const { dir, storeDir } = await setupProject('with-aliased-dep')

    const { output, exitCode } = await licenses.handler({
      ...DEFAULT_OPTS,
      dir,
      pnpmHomeDir: '',
      json: true,
      storeDir,
      licenses: {
        disallowed: ['MIT'],
        mode: 'strict',
        depth: 'shallow',
      },
    }, ['check'])

    expect(exitCode).toBe(1)
    const parsed = JSON.parse(output)
    // The violation is reported under the real package name ('is-positive'),
    // not the manifest's alias key ('positive') — proving the shallow filter
    // matched via the lockfile-resolved identity, not the raw dependency key.
    expect(parsed.violations).toHaveLength(1)
    expect(parsed.violations[0].packageName).toBe('is-positive')
  })
})
