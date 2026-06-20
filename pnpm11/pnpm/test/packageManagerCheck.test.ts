import { describe, expect, test } from '@jest/globals'
import { prepare, prepareEmpty } from '@pnpm/prepare'
import { writeYamlFileSync } from 'write-yaml-file'

import { execPnpmSync } from './utils/index.js'

test('install should fail if the used pnpm version does not satisfy the pnpm version specified in engines', async () => {
  prepare({
    name: 'project',
    version: '1.0.0',

    engines: {
      pnpm: '99999',
    },
  })

  const { status, stdout } = execPnpmSync(['install'])

  expect(status).toBe(1)
  expect(stdout.toString()).toContain('Your pnpm version is incompatible with')
})

test('install should not fail if the used pnpm version does not satisfy the pnpm version specified in packageManager', async () => {
  prepare({
    name: 'project',
    version: '1.0.0',

    packageManager: 'pnpm@0.0.0',
  })

  expect(execPnpmSync(['install', '--pm-on-fail=ignore']).status).toBe(0)

  const { status, stderr } = execPnpmSync(['install', '--pm-on-fail=error'])

  expect(status).toBe(1)
  expect(stderr.toString()).toContain('This project is configured to use 0.0.0 of pnpm. Your current pnpm is')
})

test('install should fail if the project requires a different package manager', async () => {
  prepare({
    name: 'project',
    version: '1.0.0',

    packageManager: 'yarn@4.0.0',
  })

  const { status, stderr } = execPnpmSync(['install'])

  expect(status).toBe(1)
  expect(stderr.toString()).toContain('This project is configured to use yarn')

  expect(execPnpmSync(['install', '--pm-on-fail=warn']).status).toBe(0)
})

test('install should not fail for packageManager field with hash', async () => {
  const versionProcess = execPnpmSync(['--version'])
  const pnpmVersion = versionProcess.stdout.toString().trim()

  prepare({
    name: 'project',
    version: '1.0.0',

    packageManager: `pnpm@${pnpmVersion}+sha256.123456789`,
  })

  const { status } = execPnpmSync(['install'])
  expect(status).toBe(0)
})

test('install should not fail for packageManager field with url', async () => {
  prepare({
    name: 'project',
    version: '1.0.0',

    packageManager: 'pnpm@https://github.com/pnpm/pnpm',
  })

  const { status } = execPnpmSync(['install'])
  expect(status).toBe(0)
})

test('some commands should not fail if the required package manager is not pnpm', async () => {
  prepare({
    name: 'project',
    version: '1.0.0',

    packageManager: 'yarn@3.0.0',
  })

  const { status } = execPnpmSync(['store', 'path'])
  expect(status).toBe(0)
})

test('devEngines.packageManager with onFail=error should fail on version mismatch', async () => {
  prepare({
    devEngines: {
      packageManager: {
        name: 'pnpm',
        version: '0.0.1',
        onFail: 'error',
      },
    },
  })

  const { status, stderr } = execPnpmSync(['install'])

  expect(status).toBe(1)
  expect(stderr.toString()).toContain('This project is configured to use 0.0.1 of pnpm')
})

test('devEngines.packageManager with onFail=warn should warn on version mismatch', async () => {
  prepare({
    devEngines: {
      packageManager: {
        name: 'pnpm',
        version: '0.0.1',
        onFail: 'warn',
      },
    },
  })

  const { status, stdout } = execPnpmSync(['install'])

  expect(status).toBe(0)
  expect(stdout.toString()).toContain('This project is configured to use 0.0.1 of pnpm')
})

test('devEngines.packageManager with onFail=ignore should not check version', async () => {
  prepare({
    devEngines: {
      packageManager: {
        name: 'pnpm',
        version: '0.0.1',
        onFail: 'ignore',
      },
    },
  })

  const { status, stdout, stderr } = execPnpmSync(['install'])

  expect(status).toBe(0)
  expect(stdout.toString()).not.toContain('0.0.1')
  expect(stderr.toString()).not.toContain('0.0.1')
})

test('devEngines.runtime with onFail=error should fail on Node.js version mismatch', async () => {
  prepare({
    devEngines: {
      runtime: {
        name: 'node',
        version: '99999.0.0',
        onFail: 'error',
      },
    },
  })

  const { status, stderr } = execPnpmSync(['--config.verify-deps-before-run=false', 'exec', 'node', '--version'])

  expect(status).toBe(1)
  expect(stderr.toString()).toContain('This project requires Node.js 99999.0.0')
})

test('devEngines.runtime with onFail=warn should warn on Node.js version mismatch', async () => {
  prepare({
    devEngines: {
      runtime: {
        name: 'node',
        version: '99999.0.0',
        onFail: 'warn',
      },
    },
  })

  const { status, stdout, stderr } = execPnpmSync(['--config.verify-deps-before-run=false', 'exec', 'node', '--version'])
  const output = stdout.toString() + stderr.toString()

  expect(status).toBe(0)
  expect(output).toContain('This project requires Node.js 99999.0.0')
})

test('devEngines.runtime with onFail=ignore should not check Node.js version', async () => {
  prepare({
    devEngines: {
      runtime: {
        name: 'node',
        version: '99999.0.0',
        onFail: 'ignore',
      },
    },
  })

  const { status, stdout, stderr } = execPnpmSync(['--config.verify-deps-before-run=false', 'exec', 'node', '--version'])
  const output = stdout.toString() + stderr.toString()

  expect(status).toBe(0)
  expect(output).not.toContain('99999.0.0')
})

test('engines.runtime with onFail=error should fail on Node.js version mismatch', async () => {
  prepare({
    engines: {
      runtime: {
        name: 'node',
        version: '99999.0.0',
        onFail: 'error',
      },
    },
  })

  const { status, stderr } = execPnpmSync(['--config.verify-deps-before-run=false', 'exec', 'node', '--version'])

  expect(status).toBe(1)
  expect(stderr.toString()).toContain('This project requires Node.js 99999.0.0')
})

test('engines.runtime with onFail=warn should warn on Node.js version mismatch', async () => {
  prepare({
    engines: {
      runtime: {
        name: 'node',
        version: '99999.0.0',
        onFail: 'warn',
      },
    },
  })

  const { status, stdout, stderr } = execPnpmSync(['--config.verify-deps-before-run=false', 'exec', 'node', '--version'])
  const output = stdout.toString() + stderr.toString()

  expect(status).toBe(0)
  expect(output).toContain('This project requires Node.js 99999.0.0')
})

test('engines.runtime with onFail=error should fail on invalid Node.js version range', async () => {
  prepare({
    engines: {
      runtime: {
        name: 'node',
        version: 'invalid range',
        onFail: 'error',
      },
    },
  })

  const { status, stderr } = execPnpmSync(['--config.verify-deps-before-run=false', 'exec', 'node', '--version'])

  expect(status).toBe(1)
  expect(stderr.toString()).toContain('This project requires an invalid Node.js version range: invalid range')
})

test('devEngines.runtime with onFail=error should fail on invalid Node.js version range', async () => {
  prepare({
    devEngines: {
      runtime: {
        name: 'node',
        version: 'invalid range',
        onFail: 'error',
      },
    },
  })

  const { status, stderr } = execPnpmSync(['--config.verify-deps-before-run=false', 'exec', 'node', '--version'])

  expect(status).toBe(1)
  expect(stderr.toString()).toContain('This project requires an invalid Node.js version range: invalid range')
  expect(stderr.toString()).toContain('--runtime-on-fail=ignore')
})

test('devEngines.runtime with onFail=warn should warn on invalid Node.js version range', async () => {
  prepare({
    devEngines: {
      runtime: {
        name: 'node',
        version: 'invalid range',
        onFail: 'warn',
      },
    },
  })

  const { status, stdout, stderr } = execPnpmSync(['--config.verify-deps-before-run=false', 'exec', 'node', '--version'])
  const output = stdout.toString() + stderr.toString()

  expect(status).toBe(0)
  expect(output).toContain('This project requires an invalid Node.js version range: invalid range')
})

test('devEngines.runtime with onFail=error should fail when Node.js version range is missing', async () => {
  prepare({
    devEngines: {
      runtime: {
        name: 'node',
        onFail: 'error',
      },
    },
  })

  const { status, stderr } = execPnpmSync(['--config.verify-deps-before-run=false', 'exec', 'node', '--version'])

  expect(status).toBe(1)
  expect(stderr.toString()).toContain('This project requires a Node.js runtime but does not specify a version range')
})

test('devEngines.runtime with onFail=error should fail on invalid Deno version range', async () => {
  prepare({
    devEngines: {
      runtime: {
        name: 'deno',
        version: 'invalid range',
        onFail: 'error',
      },
    },
  })

  const { status, stderr } = execPnpmSync(['--config.verify-deps-before-run=false', 'exec', 'node', '--version'])

  expect(status).toBe(1)
  expect(stderr.toString()).toContain('This project requires an invalid Deno version range: invalid range')
})

test('devEngines.runtime with onFail=error should fail on invalid Bun version range', async () => {
  prepare({
    devEngines: {
      runtime: {
        name: 'bun',
        version: 'invalid range',
        onFail: 'error',
      },
    },
  })

  const { status, stderr } = execPnpmSync(['--config.verify-deps-before-run=false', 'exec', 'node', '--version'])

  expect(status).toBe(1)
  expect(stderr.toString()).toContain('This project requires an invalid Bun version range: invalid range')
})

test('devEngines.runtime array entries are checked beyond the first one', async () => {
  prepare({
    devEngines: {
      runtime: [
        {
          name: 'node',
          version: '*',
          onFail: 'error',
        },
        {
          name: 'deno',
          version: 'invalid range',
          onFail: 'error',
        },
      ],
    },
  })

  const { status, stderr } = execPnpmSync(['--config.verify-deps-before-run=false', 'exec', 'node', '--version'])

  expect(status).toBe(1)
  expect(stderr.toString()).toContain('This project requires an invalid Deno version range: invalid range')
})

test('devEngines.runtime with onFail=ignore should not check Deno version', async () => {
  prepare({
    devEngines: {
      runtime: {
        name: 'deno',
        version: 'invalid range',
        onFail: 'ignore',
      },
    },
  })

  const { status, stdout, stderr } = execPnpmSync(['--config.verify-deps-before-run=false', 'exec', 'node', '--version'])
  const output = stdout.toString() + stderr.toString()

  expect(status).toBe(0)
  expect(output).not.toContain('invalid Deno version range')
})

test('devEngines.runtime with onFail=error should not block version output', async () => {
  prepare({
    devEngines: {
      runtime: {
        name: 'node',
        version: '99999.0.0',
        onFail: 'error',
      },
    },
  })

  const { status, stdout, stderr } = execPnpmSync(['--version'])

  expect(status).toBe(0)
  expect(stdout.toString()).toMatch(/\d+\.\d+\.\d+/)
  expect(stderr.toString()).not.toContain('This project requires Node.js 99999.0.0')
})

test('devEngines.packageManager defaults to onFail=download (#11676)', async () => {
  prepare({
    devEngines: {
      packageManager: {
        name: 'pnpm',
        version: '0.0.1',
      },
    },
  })

  // The documented `pmOnFail` default is `download`. Run under COREPACK_ROOT
  // to short-circuit the actual version switch (corepack owns version
  // selection there), so the test exercises the resolved default without a
  // network round-trip. Pre-fix, devEngines.packageManager defaulted to
  // `error` and the corepack-specific download-fallthrough hint did NOT
  // appear. Asserting on that hint pins the new behavior.
  const { status, stderr } = execPnpmSync(['install'], {
    env: { COREPACK_ROOT: '/fake/corepack' },
  })

  expect(status).toBe(1)
  expect(stderr.toString()).toContain('This project is configured to use 0.0.1 of pnpm')
  expect(stderr.toString()).toContain('does not switch versions when running under corepack')
})

test('devEngines.packageManager with a different PM name should fail with onFail=error', async () => {
  prepare({
    devEngines: {
      packageManager: {
        name: 'yarn',
        version: '>=4.0.0',
        onFail: 'error',
      },
    },
  })

  const { status, stderr } = execPnpmSync(['install'])

  expect(status).toBe(1)
  expect(stderr.toString()).toContain('This project is configured to use yarn')
})

test('devEngines.packageManager array selects the pnpm entry', async () => {
  prepare({
    devEngines: {
      packageManager: [
        { name: 'yarn', version: '>=4.0.0', onFail: 'ignore' },
        { name: 'pnpm', version: '0.0.1', onFail: 'error' },
      ],
    },
  })

  const { status, stderr } = execPnpmSync(['install'])

  expect(status).toBe(1)
  expect(stderr.toString()).toContain('This project is configured to use 0.0.1 of pnpm')
})

test('devEngines.packageManager array defaults onFail to ignore for non-last elements', async () => {
  const versionProcess = execPnpmSync(['--version'])
  const pnpmVersion = versionProcess.stdout.toString().trim()
  prepare({
    devEngines: {
      packageManager: [
        { name: 'pnpm', version: pnpmVersion },
        { name: 'yarn', version: '>=4.0.0' },
      ],
    },
  })

  // pnpm is the first (non-last) element, so onFail defaults to 'ignore'
  const { status } = execPnpmSync(['install'])

  expect(status).toBe(0)
})

test('devEngines.packageManager takes precedence over packageManager field', async () => {
  const versionProcess = execPnpmSync(['--version'])
  const pnpmVersion = versionProcess.stdout.toString().trim()
  prepare({
    packageManager: `pnpm@${pnpmVersion}`,
    devEngines: {
      packageManager: {
        name: 'pnpm',
        version: '0.0.1',
        onFail: 'error',
      },
    },
  })

  const { status, stderr } = execPnpmSync(['install'])

  // devEngines.packageManager takes effect, so version mismatch error is thrown
  expect(status).toBe(1)
  expect(stderr.toString()).toContain('This project is configured to use 0.0.1 of pnpm')
  expect(stderr.toString()).toContain('"packageManager" will be ignored')
})

test('no warning when packageManager and devEngines.packageManager specify the same exact version', async () => {
  prepare({
    packageManager: 'pnpm@1.2.3',
    devEngines: {
      packageManager: {
        name: 'pnpm',
        version: '1.2.3',
        onFail: 'ignore',
      },
    },
  })

  const { stderr } = execPnpmSync(['install'])

  expect(stderr.toString()).not.toContain('"packageManager" will be ignored')
})

test('no warning when packageManager and devEngines.packageManager specify the same version with the same integrity hash', async () => {
  const hash = 'sha512.93f7b57422ea7068257235b4c16eb60762eb68e1dc23723199cc739043ea9be2c4143274a399d8c6defa2b1176226d9ca1c4b63482d6200c1a8fbaa78c1d1485'
  prepare({
    packageManager: `pnpm@1.2.3+${hash}`,
    devEngines: {
      packageManager: {
        name: 'pnpm',
        version: `1.2.3+${hash}`,
        onFail: 'ignore',
      },
    },
  })

  const { stderr } = execPnpmSync(['install'])

  expect(stderr.toString()).not.toContain('"packageManager" will be ignored')
})

test('warns when packageManager and devEngines.packageManager pin the same version but different integrity hashes', async () => {
  prepare({
    packageManager: 'pnpm@1.2.3+sha512.aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
    devEngines: {
      packageManager: {
        name: 'pnpm',
        version: '1.2.3+sha512.bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb',
        onFail: 'ignore',
      },
    },
  })

  const { stderr } = execPnpmSync(['install'])

  expect(stderr.toString()).toContain('different integrity hashes')
  expect(stderr.toString()).toContain('"packageManager" will be ignored')
})

test('warns when the integrity hash is present on packageManager but not devEngines.packageManager', async () => {
  const hash = 'sha512.93f7b57422ea7068257235b4c16eb60762eb68e1dc23723199cc739043ea9be2c4143274a399d8c6defa2b1176226d9ca1c4b63482d6200c1a8fbaa78c1d1485'
  prepare({
    packageManager: `pnpm@1.2.3+${hash}`,
    devEngines: {
      packageManager: {
        name: 'pnpm',
        version: '1.2.3',
        onFail: 'ignore',
      },
    },
  })

  const { stderr } = execPnpmSync(['install'])

  expect(stderr.toString()).toContain('Cannot use both "packageManager" and "devEngines.packageManager"')
  expect(stderr.toString()).toContain('"packageManager" will be ignored')
})

test('warns when packageManager specifies a different package manager from devEngines.packageManager', async () => {
  prepare({
    packageManager: 'yarn@1.2.3',
    devEngines: {
      packageManager: {
        name: 'pnpm',
        version: '1.2.3',
        onFail: 'ignore',
      },
    },
  })

  const { stderr } = execPnpmSync(['install'])

  expect(stderr.toString()).toContain('specify different package managers')
  expect(stderr.toString()).toContain('"packageManager" will be ignored')
})

test('warns when packageManager version does not match the devEngines.packageManager version string exactly', async () => {
  prepare({
    packageManager: 'pnpm@1.2.3',
    devEngines: {
      packageManager: {
        name: 'pnpm',
        version: '>=1.0.0',
        onFail: 'ignore',
      },
    },
  })

  const { stderr } = execPnpmSync(['install'])

  expect(stderr.toString()).toContain('specify different versions of pnpm')
  expect(stderr.toString()).toContain('"packageManager" will be ignored')
})

test('pmOnFail=ignore via env var bypasses the devEngines.packageManager check', async () => {
  prepare({
    devEngines: {
      packageManager: {
        name: 'pnpm',
        version: '0.0.1',
        onFail: 'error',
      },
    },
  })

  const { status, stderr } = execPnpmSync(['install'], {
    env: { pnpm_config_pm_on_fail: 'ignore' },
  })

  expect(status).toBe(0)
  expect(stderr.toString()).not.toContain('0.0.1')
})

test('pmOnFail via --pm-on-fail CLI flag bypasses the devEngines.packageManager check', async () => {
  prepare({
    devEngines: {
      packageManager: {
        name: 'pnpm',
        version: '0.0.1',
        onFail: 'error',
      },
    },
  })

  expect(execPnpmSync(['install', '--pm-on-fail=ignore']).status).toBe(0)
  expect(execPnpmSync(['install', '--config.pm-on-fail=ignore']).status).toBe(0)
})

test('devEngines.packageManager check runs even when pnpm is invoked via corepack', async () => {
  prepare({
    devEngines: {
      packageManager: {
        name: 'pnpm',
        version: '0.0.1',
        onFail: 'warn',
      },
    },
  })

  // COREPACK_ROOT signals corepack-managed invocation. The package-manager
  // handling block (check + lockfile sync) used to be guarded out entirely
  // when this was set, leaving packageManagerDependencies stale (#11397).
  // The check (and sync) must run regardless of how pnpm was invoked, since
  // different developers on the same project may use either path.
  const { status, stdout } = execPnpmSync(['install'], {
    env: { COREPACK_ROOT: '/fake/corepack' },
  })

  expect(status).toBe(0)
  expect(stdout.toString()).toContain('This project is configured to use 0.0.1 of pnpm')
  // Make sure the warning explains that pnpm did not switch the version
  // because corepack is in charge — otherwise the warning is confusing.
  expect(stdout.toString()).toContain('Corepack invoked pnpm')
})

test('devEngines.packageManager onFail=download surfaces a regular error under corepack instead of switching versions', async () => {
  prepare({
    devEngines: {
      packageManager: {
        name: 'pnpm',
        version: '0.0.1',
        onFail: 'download',
      },
    },
  })

  // Corepack owns version selection, so pnpm should not attempt a version
  // switch when COREPACK_ROOT is set. Mismatches fall through to the regular
  // check, which treats onFail=download as shouldError=true. The error
  // message must spell out that pnpm did not switch the version *because*
  // corepack is in charge — otherwise the user sees a download-on-mismatch
  // contract that silently failed to download.
  const { status, stderr } = execPnpmSync(['install'], {
    env: { COREPACK_ROOT: '/fake/corepack' },
  })

  expect(status).toBe(1)
  expect(stderr.toString()).toContain('This project is configured to use 0.0.1 of pnpm')
  expect(stderr.toString()).toContain('Corepack invoked pnpm')
  expect(stderr.toString()).toContain('does not switch versions when running under corepack')
  expect(stderr.toString()).toContain('invoke pnpm directly')
})

test('pmOnFail=ignore set in pnpm-workspace.yaml bypasses the devEngines.packageManager check', async () => {
  prepare({
    devEngines: {
      packageManager: {
        name: 'pnpm',
        version: '0.0.1',
        onFail: 'error',
      },
    },
  })
  writeYamlFileSync('pnpm-workspace.yaml', {
    pmOnFail: 'ignore',
  })

  const { status, stderr } = execPnpmSync(['install'])

  expect(status).toBe(0)
  expect(stderr.toString()).not.toContain('0.0.1')
})

// Regression for #11487. The --version and --help short-circuits in
// parse-cli-args used to drop every parsed option, so `--pm-on-fail=ignore`
// silently disappeared whenever it was combined with `--version` or
// `--help` — leaving users with no way to opt out of the strict
// packageManager check just to read help or check the running version.
test.each([
  [['--pm-on-fail=ignore', '--version']],
  [['--version', '--pm-on-fail=ignore']],
  [['audit', '--pm-on-fail=ignore', '--help']],
  [['audit', '--help', '--pm-on-fail=ignore']],
])('--pm-on-fail=ignore is honored when combined with --version/--help: %p', (args) => {
  prepare({
    packageManager: 'pnpm@0.0.1',
    devEngines: {
      packageManager: {
        name: 'pnpm',
        version: '0.0.1',
        onFail: 'error',
      },
    },
  })

  const { status, stderr } = execPnpmSync(args)

  expect(status).toBe(0)
  expect(stderr.toString()).not.toContain('configured to use 0.0.1')
})

// These tests resolve the running pnpm version's integrity from registry-mock,
// which proxies pnpm to npmjs. They fail between a release commit and the
// matching npm publish ("No matching version found for pnpm@<version>"), and
// pass again once the version lands on npmjs.
describe('release-brittle: may fail until current version is published to npm', () => {
  test('pnpm --version exits promptly when devEngines.packageManager matches the running pnpm', async () => {
    // Regression test: main.ts's `--version` short-circuit returned before
    // the command-handler `finally` that calls finishWorkers(), and
    // switchCliVersion had already spawned workers during integrity
    // resolution. The worker pool then kept the Node event loop alive long
    // past the version print.
    // Read the running pnpm version from a fresh empty dir — the previous
    // test's prepare() leaves cwd in a manifest with a failing pm check, and
    // checkPackageManager runs before the --version short-circuit.
    prepareEmpty()
    const versionProcess = execPnpmSync(['--version'])
    const pnpmVersion = versionProcess.stdout.toString().trim()

    prepare({
      devEngines: {
        packageManager: {
          name: 'pnpm',
          version: pnpmVersion,
          onFail: 'download',
        },
      },
    })

    // 30 s is comfortably above the post-fix exit time (~3 s) and far below
    // the pre-fix hang. If the regression returns, spawnSync's timeout kicks
    // in and execPnpmSync throws from its `error`/`signal` checks.
    const { status, stdout } = execPnpmSync(['--version'], { timeout: 30_000 })

    expect(status).toBe(0)
    expect(stdout.toString().trim()).toBe(pnpmVersion)
  })

  test('devEngines.packageManager with version range should match current version', async () => {
    prepare({
      devEngines: {
        packageManager: {
          name: 'pnpm',
          version: '>=1.0.0',
          onFail: 'error',
        },
      },
    })

    const { status } = execPnpmSync(['install'])

    expect(status).toBe(0)
  })
})
