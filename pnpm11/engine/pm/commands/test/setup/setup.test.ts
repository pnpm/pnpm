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
