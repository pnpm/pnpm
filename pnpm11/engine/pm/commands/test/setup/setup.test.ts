import os from 'node:os'
import path from 'node:path'

import { afterEach, beforeEach, expect, jest, test } from '@jest/globals'
import { PnpmError } from '@pnpm/error'
import type { PathExtenderReport } from '@pnpm/os.env.path-extender'

jest.unstable_mockModule('@pnpm/os.env.path-extender', () => ({
  addDirToEnvPath: jest.fn(),
}))

const actualCliMeta = await import('@pnpm/cli.meta')
jest.unstable_mockModule('@pnpm/cli.meta', () => ({
  ...actualCliMeta,
  detectIfCurrentPkgIsExecutable: jest.fn(() => false),
}))

const actualChildProcess = await import('node:child_process')
jest.unstable_mockModule('node:child_process', () => ({
  ...actualChildProcess,
  spawnSync: jest.fn(() => ({ status: 0 })),
}))

const actualFs = await import('node:fs')
jest.unstable_mockModule('fs', () => {
  return {
    ...actualFs,
    promises: {
      ...actualFs.promises,
      readFile: jest.fn(),
      writeFile: jest.fn(),
    },
  }
})

const { addDirToEnvPath } = await import('@pnpm/os.env.path-extender')
const { detectIfCurrentPkgIsExecutable } = await import('@pnpm/cli.meta')
const { spawnSync } = await import('node:child_process')
const { setup, LEGACY_HOME_DIR_SHIM_NAMES } = await import('@pnpm/engine.pm.commands')

const originalGithubActions = process.env.GITHUB_ACTIONS
const originalGithubEnv = process.env.GITHUB_ENV
const originalGithubPath = process.env.GITHUB_PATH

function restoreEnvVar (name: string, value: string | undefined): void {
  if (value == null) {
    delete process.env[name]
  } else {
    process.env[name] = value
  }
}

beforeEach(() => {
  delete process.env.GITHUB_ACTIONS
  delete process.env.GITHUB_ENV
  delete process.env.GITHUB_PATH
})

afterEach(() => {
  restoreEnvVar('GITHUB_ACTIONS', originalGithubActions)
  restoreEnvVar('GITHUB_ENV', originalGithubEnv)
  restoreEnvVar('GITHUB_PATH', originalGithubPath)
})

test('setup makes no changes', async () => {
  jest.mocked(addDirToEnvPath).mockReturnValue(Promise.resolve<PathExtenderReport>({
    oldSettings: 'PNPM_HOME=dir',
    newSettings: 'PNPM_HOME=dir',
  }))
  const output = await setup.handler({ pnpmHomeDir: '' })
  expect(output).toBe('No changes to the environment were made. Everything is already up to date.')
})

test('setup makes changes on POSIX', async () => {
  jest.mocked(addDirToEnvPath).mockReturnValue(Promise.resolve<PathExtenderReport>({
    configFile: {
      changeType: 'created',
      path: '~/.bashrc',
    },
    oldSettings: 'export PNPM_HOME=dir1',
    newSettings: 'export PNPM_HOME=dir2',
  }))
  const output = await setup.handler({ pnpmHomeDir: '' })
  expect(output).toBe(`Created ~/.bashrc

Next configuration changes were made:
export PNPM_HOME=dir2

To start using pnpm, run:
source ~/.bashrc
`)
})

test('setup makes changes on Windows', async () => {
  jest.mocked(addDirToEnvPath).mockReturnValue(Promise.resolve<PathExtenderReport>({
    oldSettings: 'export PNPM_HOME=dir1',
    newSettings: 'export PNPM_HOME=dir2',
  }))
  const output = await setup.handler({ pnpmHomeDir: '' })
  expect(output).toBe(`Next configuration changes were made:
export PNPM_HOME=dir2

Setup complete. Open a new terminal to start using pnpm.`)
})

test('setup persists PNPM_HOME and bin path for GitHub Actions', async () => {
  jest.mocked(addDirToEnvPath).mockReturnValue(Promise.resolve<PathExtenderReport>({
    oldSettings: 'PNPM_HOME=dir',
    newSettings: 'PNPM_HOME=dir',
  }))
  const tmpDir = actualFs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-setup-github-actions-'))
  const pnpmHomeDir = path.join(tmpDir, 'pnpm-home')
  const githubEnv = path.join(tmpDir, 'github-env')
  const githubPath = path.join(tmpDir, 'github-path')
  actualFs.writeFileSync(githubEnv, '')
  actualFs.writeFileSync(githubPath, '')
  process.env.GITHUB_ACTIONS = 'true'
  process.env.GITHUB_ENV = githubEnv
  process.env.GITHUB_PATH = githubPath
  try {
    await setup.handler({ pnpmHomeDir })
    expect(actualFs.readFileSync(githubEnv, 'utf8')).toBe(`PNPM_HOME=${pnpmHomeDir}\n`)
    expect(actualFs.readFileSync(githubPath, 'utf8')).toBe(`${path.join(pnpmHomeDir, 'bin')}\n`)
  } finally {
    actualFs.rmSync(tmpDir, { recursive: true, force: true })
  }
})

test('setup starts GitHub Actions env-file records on a new line', async () => {
  jest.mocked(addDirToEnvPath).mockReturnValue(Promise.resolve<PathExtenderReport>({
    oldSettings: 'PNPM_HOME=dir',
    newSettings: 'PNPM_HOME=dir',
  }))
  const tmpDir = actualFs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-setup-github-actions-'))
  const pnpmHomeDir = path.join(tmpDir, 'pnpm-home')
  const githubEnv = path.join(tmpDir, 'github-env')
  const githubPath = path.join(tmpDir, 'github-path')
  actualFs.writeFileSync(githubEnv, 'EXISTING=value')
  actualFs.writeFileSync(githubPath, '/existing/bin')
  process.env.GITHUB_ACTIONS = 'true'
  process.env.GITHUB_ENV = githubEnv
  process.env.GITHUB_PATH = githubPath
  try {
    await setup.handler({ pnpmHomeDir })
    expect(actualFs.readFileSync(githubEnv, 'utf8')).toBe(`EXISTING=value\nPNPM_HOME=${pnpmHomeDir}\n`)
    expect(actualFs.readFileSync(githubPath, 'utf8')).toBe(`/existing/bin\n${path.join(pnpmHomeDir, 'bin')}\n`)
  } finally {
    actualFs.rmSync(tmpDir, { recursive: true, force: true })
  }
})

test('setup ignores GitHub Actions env files outside GitHub Actions', async () => {
  jest.mocked(addDirToEnvPath).mockReturnValue(Promise.resolve<PathExtenderReport>({
    oldSettings: 'PNPM_HOME=dir',
    newSettings: 'PNPM_HOME=dir',
  }))
  const tmpDir = actualFs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-setup-github-actions-'))
  const pnpmHomeDir = path.join(tmpDir, 'pnpm-home')
  const githubEnv = path.join(tmpDir, 'github-env')
  const githubPath = path.join(tmpDir, 'github-path')
  actualFs.writeFileSync(githubEnv, '')
  actualFs.writeFileSync(githubPath, '')
  process.env.GITHUB_ENV = githubEnv
  process.env.GITHUB_PATH = githubPath
  try {
    await setup.handler({ pnpmHomeDir })
    expect(actualFs.readFileSync(githubEnv, 'utf8')).toBe('')
    expect(actualFs.readFileSync(githubPath, 'utf8')).toBe('')
  } finally {
    actualFs.rmSync(tmpDir, { recursive: true, force: true })
  }
})

test('setup writes each available GitHub Actions file independently', async () => {
  jest.mocked(addDirToEnvPath).mockReturnValue(Promise.resolve<PathExtenderReport>({
    oldSettings: 'PNPM_HOME=dir',
    newSettings: 'PNPM_HOME=dir',
  }))
  const tmpDir = actualFs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-setup-github-actions-'))
  const pnpmHomeDir = path.join(tmpDir, 'pnpm-home')
  const githubEnv = path.join(tmpDir, 'github-env')
  const githubPath = path.join(tmpDir, 'github-path')
  actualFs.writeFileSync(githubEnv, '')
  actualFs.writeFileSync(githubPath, '')
  process.env.GITHUB_ACTIONS = 'true'
  try {
    process.env.GITHUB_ENV = githubEnv
    await setup.handler({ pnpmHomeDir })
    expect(actualFs.readFileSync(githubEnv, 'utf8')).toBe(`PNPM_HOME=${pnpmHomeDir}\n`)

    delete process.env.GITHUB_ENV
    process.env.GITHUB_PATH = githubPath
    await setup.handler({ pnpmHomeDir })
    expect(actualFs.readFileSync(githubPath, 'utf8')).toBe(`${path.join(pnpmHomeDir, 'bin')}\n`)
  } finally {
    actualFs.rmSync(tmpDir, { recursive: true, force: true })
  }
})

test('setup does not create missing GitHub Actions env files', async () => {
  jest.mocked(addDirToEnvPath).mockReturnValue(Promise.resolve<PathExtenderReport>({
    oldSettings: 'PNPM_HOME=dir',
    newSettings: 'PNPM_HOME=dir',
  }))
  const tmpDir = actualFs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-setup-github-actions-'))
  const pnpmHomeDir = path.join(tmpDir, 'pnpm-home')
  const githubEnv = path.join(tmpDir, 'missing-github-env')
  process.env.GITHUB_ACTIONS = 'true'
  process.env.GITHUB_ENV = githubEnv
  try {
    await setup.handler({ pnpmHomeDir })
    expect(actualFs.existsSync(githubEnv)).toBeFalsy()
  } finally {
    actualFs.rmSync(tmpDir, { recursive: true, force: true })
  }
})

test('setup skips GitHub Actions env files that are not regular files', async () => {
  jest.mocked(addDirToEnvPath).mockReturnValue(Promise.resolve<PathExtenderReport>({
    oldSettings: 'PNPM_HOME=dir',
    newSettings: 'PNPM_HOME=dir',
  }))
  const tmpDir = actualFs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-setup-github-actions-'))
  const pnpmHomeDir = path.join(tmpDir, 'pnpm-home')
  const githubEnv = path.join(tmpDir, 'github-env-dir')
  const githubPath = path.join(tmpDir, 'github-path')
  actualFs.mkdirSync(githubEnv)
  actualFs.writeFileSync(githubPath, '')
  process.env.GITHUB_ACTIONS = 'true'
  process.env.GITHUB_ENV = githubEnv
  process.env.GITHUB_PATH = githubPath
  try {
    await setup.handler({ pnpmHomeDir })
    expect(actualFs.readdirSync(githubEnv)).toStrictEqual([])
    expect(actualFs.readFileSync(githubPath, 'utf8')).toBe(`${path.join(pnpmHomeDir, 'bin')}\n`)
  } finally {
    actualFs.rmSync(tmpDir, { recursive: true, force: true })
  }
})

test('setup writes the remaining GitHub Actions env files after one target fails', async () => {
  jest.mocked(addDirToEnvPath).mockReturnValue(Promise.resolve<PathExtenderReport>({
    oldSettings: 'PNPM_HOME=dir',
    newSettings: 'PNPM_HOME=dir',
  }))
  const tmpDir = actualFs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-setup-github-actions-'))
  const pnpmHomeDir = path.join(tmpDir, 'pnpm-home')
  const githubEnv = path.join(tmpDir, 'a'.repeat(300))
  const githubPath = path.join(tmpDir, 'github-path')
  actualFs.writeFileSync(githubPath, '')
  process.env.GITHUB_ACTIONS = 'true'
  process.env.GITHUB_ENV = githubEnv
  process.env.GITHUB_PATH = githubPath
  try {
    await setup.handler({ pnpmHomeDir })
    expect(actualFs.readFileSync(githubPath, 'utf8')).toBe(`${path.join(pnpmHomeDir, 'bin')}\n`)
  } finally {
    actualFs.rmSync(tmpDir, { recursive: true, force: true })
  }
})

test('setup rejects GitHub Actions env-file values with line-breaking characters', async () => {
  jest.mocked(addDirToEnvPath).mockReturnValue(Promise.resolve<PathExtenderReport>({
    oldSettings: 'PNPM_HOME=dir',
    newSettings: 'PNPM_HOME=dir',
  }))
  const tmpDir = actualFs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-setup-github-actions-'))
  const githubEnv = path.join(tmpDir, 'github-env')
  actualFs.writeFileSync(githubEnv, '')
  process.env.GITHUB_ACTIONS = 'true'
  process.env.GITHUB_ENV = githubEnv
  try {
    await expect(setup.handler({ pnpmHomeDir: `${tmpDir}\nINJECTED=value` }))
      .rejects.toMatchObject({ code: 'ERR_PNPM_BAD_GITHUB_ACTIONS_ENVIRONMENT_VALUE' })
  } finally {
    actualFs.rmSync(tmpDir, { recursive: true, force: true })
  }
})

test('hint is added to ERR_PNPM_BAD_ENV_FOUND error object', async () => {
  jest.mocked(addDirToEnvPath).mockReturnValue(Promise.reject(new PnpmError('BAD_ENV_FOUND', '')))
  let err!: PnpmError
  try {
    await setup.handler({ pnpmHomeDir: '' })
  } catch (_err: any) { // eslint-disable-line
    err = _err
  }
  expect(err?.hint).toBe('If you want to override the existing env variable, use the --force option')
})

test('hint is added to ERR_PNPM_BAD_SHELL_SECTION error object', async () => {
  jest.mocked(addDirToEnvPath).mockReturnValue(Promise.reject(new PnpmError('BAD_SHELL_SECTION', '')))
  let err!: PnpmError
  try {
    await setup.handler({ pnpmHomeDir: '' })
  } catch (_err: any) { // eslint-disable-line
    err = _err
  }
  expect(err?.hint).toBe('If you want to override the existing configuration section, use the --force option')
})

test('global install of the standalone executable skips its build scripts', async () => {
  jest.mocked(addDirToEnvPath).mockReturnValue(Promise.resolve<PathExtenderReport>({
    oldSettings: 'PNPM_HOME=dir',
    newSettings: 'PNPM_HOME=dir',
  }))
  jest.mocked(detectIfCurrentPkgIsExecutable).mockReturnValue(true)
  const tmpDir = actualFs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-setup-test-'))
  const execPath = path.join(tmpDir, 'pnpm')
  const originalExecPath = process.execPath
  Object.defineProperty(process, 'execPath', { value: execPath, configurable: true })
  try {
    await setup.handler({ pnpmHomeDir: path.join(tmpDir, 'home') })
  } finally {
    Object.defineProperty(process, 'execPath', { value: originalExecPath, configurable: true })
    actualFs.rmSync(tmpDir, { recursive: true, force: true })
  }
  expect(spawnSync).toHaveBeenCalledTimes(1)
  const args = jest.mocked(spawnSync).mock.calls[0][1] as string[]
  expect(args).toContain('--ignore-scripts')
  expect(args).toEqual(['add', '-g', '--ignore-scripts', `file:${tmpDir}`])
})

test('the manifest written next to the standalone executable declares its package files', () => {
  expect(setup.standaloneManifest('pnpm.exe')).toStrictEqual({
    name: '@pnpm/exe',
    version: actualCliMeta.packageManager.version,
    type: 'module',
    bin: { pnpm: 'pnpm.exe', pn: 'pnpm.exe' },
    files: ['pnpm.exe', 'dist/'],
  })
})

test('setup removes leftover v10-layout shims at the top of pnpmHomeDir', async () => {
  // Reproduces pnpm/pnpm#12496: `pnpm setup` migrated PATH to
  // pnpmHomeDir/bin but left the v10-layout shims (pnpm/pn/pnpx/pnx and
  // .cmd/.ps1 siblings) at pnpmHomeDir itself. self-update keyed off their
  // mere existence and re-warned about a v10 layout forever.
  jest.mocked(addDirToEnvPath).mockReturnValue(Promise.resolve<PathExtenderReport>({
    oldSettings: 'PNPM_HOME=dir',
    newSettings: 'PNPM_HOME=dir',
  }))
  jest.mocked(detectIfCurrentPkgIsExecutable).mockReturnValue(true)
  const tmpDir = actualFs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-setup-test-'))
  const pnpmHomeDir = path.join(tmpDir, 'home')
  actualFs.mkdirSync(pnpmHomeDir, { recursive: true })
  // Pre-create the full set of v10-layout shim names. The cleanup must
  // tolerate names that don't exist (e.g. .ps1 on POSIX) without error.
  for (const name of LEGACY_HOME_DIR_SHIM_NAMES) {
    actualFs.writeFileSync(path.join(pnpmHomeDir, name), 'stale shim\n')
  }
  const execPath = path.join(tmpDir, 'pnpm')
  const originalExecPath = process.execPath
  Object.defineProperty(process, 'execPath', { value: execPath, configurable: true })
  try {
    await setup.handler({ pnpmHomeDir })
    for (const name of LEGACY_HOME_DIR_SHIM_NAMES) {
      expect(actualFs.existsSync(path.join(pnpmHomeDir, name))).toBe(false)
    }
  } finally {
    Object.defineProperty(process, 'execPath', { value: originalExecPath, configurable: true })
    actualFs.rmSync(tmpDir, { recursive: true, force: true })
  }
})

test('setup ignores legacy shim cleanup failures', async () => {
  jest.mocked(addDirToEnvPath).mockReturnValue(Promise.resolve<PathExtenderReport>({
    oldSettings: 'PNPM_HOME=dir',
    newSettings: 'PNPM_HOME=dir',
  }))
  jest.mocked(detectIfCurrentPkgIsExecutable).mockReturnValue(true)
  const tmpDir = actualFs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-setup-test-'))
  const pnpmHomeDir = path.join(tmpDir, 'home')
  actualFs.mkdirSync(pnpmHomeDir, { recursive: true })
  actualFs.mkdirSync(path.join(pnpmHomeDir, LEGACY_HOME_DIR_SHIM_NAMES[0]))
  const removableName = LEGACY_HOME_DIR_SHIM_NAMES[1]
  actualFs.writeFileSync(path.join(pnpmHomeDir, removableName), 'stale shim\n')
  const execPath = path.join(tmpDir, 'pnpm')
  const originalExecPath = process.execPath
  Object.defineProperty(process, 'execPath', { value: execPath, configurable: true })
  try {
    await setup.handler({ pnpmHomeDir })
    expect(actualFs.existsSync(path.join(pnpmHomeDir, removableName))).toBe(false)
  } finally {
    Object.defineProperty(process, 'execPath', { value: originalExecPath, configurable: true })
    actualFs.rmSync(tmpDir, { recursive: true, force: true })
  }
})

// The aliases setup writes are `sh` scripts, so this runs where `sh` does. On
// Windows the .cmd and .ps1 wrappers take over, and they have only PATH to go on.
const posixTest = process.platform === 'win32' ? test.skip : test

posixTest('the alias scripts run the pnpm beside them, not one earlier on PATH', async () => {
  jest.mocked(addDirToEnvPath).mockReturnValue(Promise.resolve<PathExtenderReport>({
    oldSettings: 'PNPM_HOME=dir',
    newSettings: 'PNPM_HOME=dir',
  }))
  jest.mocked(detectIfCurrentPkgIsExecutable).mockReturnValue(true)
  const tmpDir = actualFs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-setup-test-'))
  const pnpmHomeDir = path.join(tmpDir, 'home')
  const execPath = path.join(tmpDir, 'pnpm')
  const originalExecPath = process.execPath
  Object.defineProperty(process, 'execPath', { value: execPath, configurable: true })
  try {
    await setup.handler({ pnpmHomeDir })

    const binDir = path.join(pnpmHomeDir, 'bin')
    writeStub(path.join(binDir, 'pnpm'), 'sibling')
    // Earlier on PATH, so it wins any lookup by name.
    const decoyDir = path.join(tmpDir, 'decoy')
    writeStub(path.join(decoyDir, 'pnpm'), 'decoy')

    for (const [name, injected] of [['pn', ''], ['pnpx', 'dlx '], ['pnx', 'dlx ']]) {
      const result = actualChildProcess.spawnSync(path.join(binDir, name), ['add', 'foo'], {
        encoding: 'utf8',
        env: { ...process.env, PATH: `${decoyDir}:/usr/bin:/bin` },
      })
      expect({ name, stdout: result.stdout }).toEqual({ name, stdout: `sibling: ${injected}add foo\n` })
    }
  } finally {
    Object.defineProperty(process, 'execPath', { value: originalExecPath, configurable: true })
    actualFs.rmSync(tmpDir, { recursive: true, force: true })
  }
})

/** An executable stand-in for pnpm at `file` that echoes `label` and its arguments. */
function writeStub (file: string, label: string): void {
  actualFs.mkdirSync(path.dirname(file), { recursive: true })
  actualFs.writeFileSync(file, `#!/bin/sh\necho "${label}: $*"\n`)
  actualFs.chmodSync(file, 0o755)
}

// The Windows counterparts of the test above. The stand-in siblings are named for
// the pnpm.cmd / pnpm.ps1 shims `pnpm add -g` links next to the aliases.
const winTest = process.platform === 'win32' ? test : test.skip
// What the stand-in shims exit with, so the wrappers are shown to hand the shim's
// status back rather than reporting their own success.
const SHIM_EXIT_CODE = 3
// cmd.exe needs System32 for its own startup. powershell.exe lives a few levels
// deeper, and it has to be on the PATH handed to the child because that is what
// Node resolves the command name against. The decoy stays first either way, which
// is what these tests turn on.
const SYSTEM32 = path.join(process.env.SystemRoot ?? 'C:\\Windows', 'System32')
const POWERSHELL_DIR = path.join(SYSTEM32, 'WindowsPowerShell', 'v1.0')

const WINDOWS_WRAPPERS = [
  {
    extension: 'cmd',
    command: 'cmd',
    argv: (script: string) => ['/c', script, 'add', 'foo'],
  },
  {
    extension: 'ps1',
    command: 'powershell',
    // -ExecutionPolicy Bypass because a runner's default policy blocks running a
    // script from disk, which is not what this is testing.
    argv: (script: string) => ['-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', script, 'add', 'foo'],
  },
]

for (const wrapper of WINDOWS_WRAPPERS) {
  winTest(`the .${wrapper.extension} wrappers call the pnpm shim beside them, not one earlier on PATH`, async () => {
    jest.mocked(addDirToEnvPath).mockReturnValue(Promise.resolve<PathExtenderReport>({
      oldSettings: 'PNPM_HOME=dir',
      newSettings: 'PNPM_HOME=dir',
    }))
    jest.mocked(detectIfCurrentPkgIsExecutable).mockReturnValue(true)
    const tmpDir = actualFs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-setup-test-'))
    const pnpmHomeDir = path.join(tmpDir, 'home')
    const execPath = path.join(tmpDir, 'pnpm.exe')
    const originalExecPath = process.execPath
    Object.defineProperty(process, 'execPath', { value: execPath, configurable: true })
    try {
      await setup.handler({ pnpmHomeDir })

      // pnpm.cmd is the only sibling shim guaranteed to be there: the bin linker
      // omits pnpm.ps1 for a package named `pnpm`, so neither wrapper may rely on
      // it. None is planted, so a wrapper reaching for one would fail here.
      const binDir = path.join(pnpmHomeDir, 'bin')
      writeShimStub(path.join(binDir, 'pnpm.cmd'), 'sibling')
      // Earlier on PATH, so it wins any lookup by name.
      const decoyDir = path.join(tmpDir, 'decoy')
      writeShimStub(path.join(decoyDir, 'pnpm.cmd'), 'decoy')

      for (const [name, injected] of [['pn', ''], ['pnpx', 'dlx '], ['pnx', 'dlx ']]) {
        const script = path.join(binDir, `${name}.${wrapper.extension}`)
        const result = actualChildProcess.spawnSync(wrapper.command, wrapper.argv(script), {
          encoding: 'utf8',
          env: { ...process.env, PATH: `${decoyDir};${SYSTEM32};${POWERSHELL_DIR}` },
        })
        // Otherwise a failure to spawn surfaces as `stdout` being undefined.
        if (result.error != null) throw result.error
        expect({ name, stdout: result.stdout.trimEnd(), status: result.status })
          .toEqual({ name, stdout: `sibling: ${injected}add foo`, status: SHIM_EXIT_CODE })
      }
    } finally {
      Object.defineProperty(process, 'execPath', { value: originalExecPath, configurable: true })
      actualFs.rmSync(tmpDir, { recursive: true, force: true })
    }
  })
}

/** A stand-in for pnpm's generated `.cmd` shim at `file`, echoing `label` and its arguments. */
function writeShimStub (file: string, label: string): void {
  actualFs.mkdirSync(path.dirname(file), { recursive: true })
  actualFs.writeFileSync(file, `@echo off\r\necho ${label}: %*\r\nexit /b ${SHIM_EXIT_CODE}\r\n`)
}
