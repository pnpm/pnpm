import path from 'path'
import fs from 'fs'
import { packageManager } from '@pnpm/cli-meta'
import { prepare } from '@pnpm/prepare'
import { getToolDirPath } from '@pnpm/tools.path'
import { sync as writeJsonFile } from 'write-json-file'
import { execPnpmSync } from './utils/index.js'
import isWindows from 'is-windows'

test('switch to the pnpm version specified in the packageManager field of package.json', async () => {
  prepare()
  const pnpmHome = path.resolve('pnpm')
  const env = { PNPM_HOME: pnpmHome }
  writeJsonFile('package.json', {
    packageManager: 'pnpm@9.3.0',
  })

  const { stdout } = execPnpmSync(['help'], { env })

  expect(stdout.toString()).toContain('Version 9.3.0')
})

test('do not switch to the pnpm version specified in the packageManager field of package.json, if manage-package-manager-versions is set to false', async () => {
  prepare()
  const pnpmHome = path.resolve('pnpm')
  const env = { PNPM_HOME: pnpmHome }
  fs.writeFileSync('.npmrc', 'manage-package-manager-versions=false')
  writeJsonFile('package.json', {
    packageManager: 'pnpm@9.3.0',
  })

  const { stdout } = execPnpmSync(['help'], { env })

  expect(stdout.toString()).not.toContain('Version 9.3.0')
})

test('do not switch to pnpm version that is specified not with a semver version', async () => {
  prepare()
  const pnpmHome = path.resolve('pnpm')
  const env = { PNPM_HOME: pnpmHome }
  writeJsonFile('package.json', {
    packageManager: 'pnpm@kevva/is-positive',
  })

  const { stdout } = execPnpmSync(['help'], { env })

  expect(stdout.toString()).toContain('Cannot switch to pnpm@kevva/is-positive')
})

test('do not switch to pnpm version that is specified starting with v', async () => {
  prepare()
  const pnpmHome = path.resolve('pnpm')
  const env = { PNPM_HOME: pnpmHome }
  writeJsonFile('package.json', {
    packageManager: 'pnpm@v9.15.5',
  })

  const { stdout } = execPnpmSync(['help'], { env })

  expect(stdout.toString()).toContain('Cannot switch to pnpm@v9.15.5: you need to specify the version as "9.15.5"')
})

test('do not switch to pnpm version when a range is specified', async () => {
  prepare()
  const pnpmHome = path.resolve('pnpm')
  const env = { PNPM_HOME: pnpmHome }
  writeJsonFile('package.json', {
    packageManager: 'pnpm@^9.3.0',
  })

  const { stdout } = execPnpmSync(['help'], { env })

  expect(stdout.toString()).toContain('Cannot switch to pnpm@^9.3.0')
})

test('commands that v10 passes through to npm keep passing through when packageManager selects pnpm v10', () => {
  prepare()
  const pnpmHome = path.resolve('pnpm')
  const env = { PNPM_HOME: pnpmHome }
  writeJsonFile('package.json', {
    packageManager: 'pnpm@10.0.0',
  })

  const { stdout } = execPnpmSync(['version', '--help'], { env })

  // npm's version help has this at the top — if we saw it, the argv[0]
  // passthrough fired as it always has on pnpm v10. See #11328; the two
  // tests below cover the pnpm v11+ cases (switching enabled and disabled).
  expect(stdout.toString()).toContain('Bump a package version')
})

test('`pnpm version` routes through switchCliVersion to v11 when packageManager selects pnpm v11+, even with a pnpm-workspace.yaml at the project root', () => {
  prepare()
  const pnpmHome = path.resolve('pnpm')
  const version = '11.0.0-rc.5'
  // Bypass the registry mock for this one install: fetching pnpm v11 (and its
  // full dep tree) through the proxy is slow and flaky, and all we need here
  // is a real pnpm v11 tarball to prove the handoff happened.
  const env = {
    PNPM_HOME: pnpmHome,
    npm_config_registry: 'https://registry.npmjs.org/',
  }
  // pnpm-workspace.yaml at the project root is what lets the install-child's
  // workspace walk-up see this dir's package.json + packageManager field.
  // Before the #11337 fix this would fork-bomb; the tool-dir assertion below
  // doubles as a fork-bomb regression check in the v11-target path.
  fs.writeFileSync('pnpm-workspace.yaml', '')
  writeJsonFile('package.json', {
    packageManager: `pnpm@${version}`,
  })

  const { stdout, stderr } = execPnpmSync(['version'], { env })
  const combined = stdout.toString() + stderr.toString()

  // The #11328 fix: v11 is wanted, so argv[0] must not passthrough to npm.
  // `Bump a package version` is the first line of `npm version --help`, so
  // its absence confirms the legacy passthrough didn't fire.
  expect(combined).not.toContain('Bump a package version')
  // installPnpmToTools is only reached from main() → switchCliVersion, so
  // the tool dir's existence is direct proof that the argv[0] passthrough
  // was skipped and the command was routed through main() instead. This
  // intentionally doesn't rely on v11 *executing* its `version` command —
  // pnpm v11 requires Node.js >= 22.13 while CI runs on 22.12, so v11
  // errors out at its own Node check rather than reaching the command.
  const toolDir = getToolDirPath({ pnpmHomeDir: pnpmHome, tool: { name: 'pnpm', version } })
  expect(fs.existsSync(path.join(toolDir, 'bin/pnpm'))).toBe(true)
})

test('npm passthrough still fires when packageManager selects pnpm v11+ but switching is disabled via .npmrc', () => {
  prepare()
  const pnpmHome = path.resolve('pnpm')
  const env = { PNPM_HOME: pnpmHome }
  // The user has pinned pnpm v11 in packageManager but opted out of version
  // switching. pnpm v10 can't hand the command off to v11's native
  // implementation, so we must preserve the legacy argv[0] passthrough —
  // otherwise `version` would be stranded in v10's main(), which never
  // implemented it natively.
  fs.writeFileSync('.npmrc', 'manage-package-manager-versions=false')
  writeJsonFile('package.json', {
    packageManager: 'pnpm@11.0.0-rc.3',
  })

  const { stdout } = execPnpmSync(['version', '--help'], { env })

  expect(stdout.toString()).toContain('Bump a package version')
})

test('switching does not fork-bomb when a pnpm-workspace.yaml at the project root is visible to the install-child (#11337 regression)', () => {
  prepare()
  const pnpmHome = path.resolve('pnpm')
  const env = { PNPM_HOME: pnpmHome }
  // pnpm-workspace.yaml at the project root is what makes the install-child's
  // workspace walk-up hit this dir's package.json, pulling in its
  // packageManager field and re-triggering switchCliVersion inside the child.
  // Without the env-var guard in installPnpmToTools, that would recurse
  // indefinitely because the target tool dir is not symlinked in yet.
  fs.writeFileSync('pnpm-workspace.yaml', '')
  writeJsonFile('package.json', {
    packageManager: 'pnpm@9.3.0',
  })

  const { stdout } = execPnpmSync(['help'], { env, timeout: 60_000 })

  expect(stdout.toString()).toContain('Version 9.3.0')
}, 90_000)

test('no spurious re-entry when the packageManager version matches the current pnpm, even with a pnpm-workspace.yaml at the root', () => {
  prepare()
  const pnpmHome = path.resolve('pnpm')
  const env = { PNPM_HOME: pnpmHome }
  // Same-version scenario: switchCliVersion must short-circuit at the
  // `pm.version === packageManager.version` check (switchCliVersion.ts). The
  // ancestor pnpm-workspace.yaml must not cause any detour through
  // installPnpmToTools. If it did, `pnpm -v` would either hang or print the
  // wrong version.
  fs.writeFileSync('pnpm-workspace.yaml', '')
  writeJsonFile('package.json', {
    packageManager: `pnpm@${packageManager.version}`,
  })

  const { stdout } = execPnpmSync(['-v'], { env, timeout: 30_000 })

  expect(stdout.toString().trim()).toBe(packageManager.version)
  // And the tool dir must not have been created — no install should have run.
  const toolDir = getToolDirPath({ pnpmHomeDir: pnpmHome, tool: { name: 'pnpm', version: packageManager.version } })
  expect(fs.existsSync(path.join(toolDir, 'bin/pnpm'))).toBe(false)
}, 60_000)

test('throws error if pnpm tools dir is corrupt', () => {
  prepare()
  const pnpmHome = path.resolve('pnpm')
  const env = { PNPM_HOME: pnpmHome }
  const version = '9.3.0'
  fs.writeFileSync('.npmrc', 'manage-package-manager-versions=true')
  writeJsonFile('package.json', {
    packageManager: `pnpm@${version}`,
  })

  // Run pnpm once to ensure the tools dir is created.
  execPnpmSync(['help'], { env })

  // Intentionally corrupt the tool dir.
  const toolDir = getToolDirPath({ pnpmHomeDir: pnpmHome, tool: { name: 'pnpm', version } })
  fs.rmSync(path.join(toolDir, 'bin/pnpm'))
  if (isWindows()) {
    fs.rmSync(path.join(toolDir, 'bin/pnpm.cmd'))
  }

  const { stderr } = execPnpmSync(['help'], { env })
  expect(stderr.toString()).toContain('Failed to switch pnpm to v9.3.0. Looks like pnpm CLI is missing')
})
