import path from 'node:path'

import { afterAll, beforeEach, expect, jest, test } from '@jest/globals'
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

const actualOs = await import('node:os')
jest.unstable_mockModule('node:os', () => {
  const homedir = jest.fn(() => actualOs.homedir())
  return {
    ...actualOs,
    default: {
      ...actualOs,
      homedir,
    },
    homedir,
  }
})

const { addDirToEnvPath } = await import('@pnpm/os.env.path-extender')
const { detectIfCurrentPkgIsExecutable } = await import('@pnpm/cli.meta')
const { spawnSync } = await import('node:child_process')
const { setup } = await import('@pnpm/engine.pm.commands')
const os = await import('node:os')

const testIfSymlinkSupported = process.platform === 'win32' ? test.skip : test

const originalFishVersion = process.env.FISH_VERSION
const originalHome = process.env.HOME
const originalShell = process.env.SHELL
const originalXdgConfigHome = process.env.XDG_CONFIG_HOME

beforeEach(() => {
  jest.clearAllMocks()
  delete process.env.FISH_VERSION
  delete process.env.XDG_CONFIG_HOME
  process.env.SHELL = '/bin/bash'
  jest.mocked(os.default.homedir).mockReturnValue(originalHome ?? actualOs.homedir())
  jest.mocked(addDirToEnvPath).mockReset()
  jest.mocked(detectIfCurrentPkgIsExecutable).mockReturnValue(false)
  jest.mocked(spawnSync).mockReturnValue({
    pid: 0,
    output: [],
    stdout: '',
    stderr: '',
    status: 0,
    signal: null,
  })
})

afterAll(() => {
  if (originalFishVersion == null) {
    delete process.env.FISH_VERSION
  } else {
    process.env.FISH_VERSION = originalFishVersion
  }
  if (originalShell == null) {
    delete process.env.SHELL
  } else {
    process.env.SHELL = originalShell
  }
  if (originalXdgConfigHome == null) {
    delete process.env.XDG_CONFIG_HOME
  } else {
    process.env.XDG_CONFIG_HOME = originalXdgConfigHome
  }
  if (originalHome == null) {
    delete process.env.HOME
  } else {
    process.env.HOME = originalHome
  }
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

test('setup writes fish config to conf.d', async () => {
  const tempHome = actualFs.mkdtempSync(path.join(actualOs.tmpdir(), 'pnpm-setup-fish-'))
  jest.mocked(os.default.homedir).mockReturnValue(tempHome)
  process.env.FISH_VERSION = '3.7.0'
  try {
    const configFile = path.join(tempHome, '.config/fish/conf.d/pnpm.fish')
    const output = await setup.handler({ pnpmHomeDir: '/pnpm-home' })
    expect(actualFs.readFileSync(configFile, 'utf8')).toBe(`set -gx PNPM_HOME "/pnpm-home"
if not string match -q -- "$PNPM_HOME/bin" $PATH
  set -gx PATH "$PNPM_HOME/bin" $PATH
end
`)
    expect(jest.mocked(addDirToEnvPath)).not.toHaveBeenCalled()
    expect(output).toBe(`Created ${configFile}

Next configuration changes were made:
set -gx PNPM_HOME "/pnpm-home"
if not string match -q -- "$PNPM_HOME/bin" $PATH
  set -gx PATH "$PNPM_HOME/bin" $PATH
end

To start using pnpm, run:
source ${configFile}
`)
  } finally {
    actualFs.rmSync(tempHome, { force: true, recursive: true })
  }
})

test('setup writes fish config under XDG_CONFIG_HOME when set', async () => {
  const tempConfigHome = actualFs.mkdtempSync(path.join(actualOs.tmpdir(), 'pnpm-setup-fish-xdg-'))
  process.env.XDG_CONFIG_HOME = tempConfigHome
  process.env.FISH_VERSION = '3.7.0'
  try {
    const configFile = path.join(tempConfigHome, 'fish/conf.d/pnpm.fish')
    const output = await setup.handler({ pnpmHomeDir: '/pnpm-home' })
    expect(actualFs.existsSync(configFile)).toBe(true)
    expect(jest.mocked(addDirToEnvPath)).not.toHaveBeenCalled()
    expect(output).toContain(`Created ${configFile}`)
  } finally {
    actualFs.rmSync(tempConfigHome, { force: true, recursive: true })
  }
})

test('setup quotes fish source command for unsafe XDG_CONFIG_HOME characters', async () => {
  const tempParent = actualFs.mkdtempSync(path.join(actualOs.tmpdir(), 'pnpm setup; $xdg-'))
  const tempConfigHome = path.join(tempParent, 'config home')
  actualFs.mkdirSync(tempConfigHome)
  process.env.XDG_CONFIG_HOME = tempConfigHome
  process.env.FISH_VERSION = '3.7.0'
  try {
    const configFile = path.join(tempConfigHome, 'fish/conf.d/pnpm.fish')
    const output = await setup.handler({ pnpmHomeDir: '/pnpm-home' })
    const quotedConfigFile = `"${configFile.replace(/\\/g, '\\\\').replace(/"/g, '\\"').replace(/\$/g, '\\$').replace(/`/g, '\\`')}"`

    expect(output).toContain(`To start using pnpm, run:
source ${quotedConfigFile}
`)
    expect(output).not.toContain(`source ${configFile}
`)
  } finally {
    actualFs.rmSync(tempParent, { force: true, recursive: true })
  }
})

test('setup rejects relative XDG_CONFIG_HOME for fish config', async () => {
  process.env.XDG_CONFIG_HOME = 'relative-config'
  process.env.FISH_VERSION = '3.7.0'

  await expect(setup.handler({ pnpmHomeDir: '/pnpm-home' })).rejects.toMatchObject({
    code: 'ERR_PNPM_UNSAFE_SHELL_CONFIG',
  })
})

test('setup rejects relative home directory fallback for fish config', async () => {
  jest.mocked(os.default.homedir).mockReturnValue('relative-home')
  process.env.FISH_VERSION = '3.7.0'

  await expect(setup.handler({ pnpmHomeDir: '/pnpm-home' })).rejects.toMatchObject({
    code: 'ERR_PNPM_UNSAFE_SHELL_CONFIG',
  })
})

test('setup escapes PNPM_HOME when writing fish config', async () => {
  const tempHome = actualFs.mkdtempSync(path.join(actualOs.tmpdir(), 'pnpm-setup-fish-escape-'))
  jest.mocked(os.default.homedir).mockReturnValue(tempHome)
  process.env.FISH_VERSION = '3.7.0'
  try {
    const configFile = path.join(tempHome, '.config/fish/conf.d/pnpm.fish')
    await setup.handler({ pnpmHomeDir: '/pnpm"$HOME\\dir' })
    expect(actualFs.readFileSync(configFile, 'utf8')).toContain('set -gx PNPM_HOME "/pnpm\\"\\$HOME\\\\dir"')
  } finally {
    actualFs.rmSync(tempHome, { force: true, recursive: true })
  }
})

test('setup rejects PNPM_HOME with control characters for fish config', async () => {
  const tempHome = actualFs.mkdtempSync(path.join(actualOs.tmpdir(), 'pnpm-setup-fish-control-'))
  jest.mocked(os.default.homedir).mockReturnValue(tempHome)
  process.env.FISH_VERSION = '3.7.0'
  try {
    await expect(setup.handler({ pnpmHomeDir: '/pnpm-home\nset -gx BAD 1' })).rejects.toMatchObject({
      code: 'ERR_PNPM_UNSAFE_SHELL_CONFIG',
    })
    await expect(setup.handler({ pnpmHomeDir: '/pnpm-home\tbad' })).rejects.toMatchObject({
      code: 'ERR_PNPM_UNSAFE_SHELL_CONFIG',
    })
    await expect(setup.handler({ pnpmHomeDir: '/pnpm-home\x1Bbad' })).rejects.toMatchObject({
      code: 'ERR_PNPM_UNSAFE_SHELL_CONFIG',
    })
    await expect(setup.handler({ pnpmHomeDir: '/pnpm-home\u0085bad' })).rejects.toMatchObject({
      code: 'ERR_PNPM_UNSAFE_SHELL_CONFIG',
    })
    expect(actualFs.existsSync(path.join(tempHome, '.config/fish/conf.d/pnpm.fish'))).toBe(false)
  } finally {
    actualFs.rmSync(tempHome, { force: true, recursive: true })
  }
})

test('setup rejects PNPM_HOME with PATH delimiters for fish config', async () => {
  const tempHome = actualFs.mkdtempSync(path.join(actualOs.tmpdir(), 'pnpm-setup-fish-path-delimiter-'))
  jest.mocked(os.default.homedir).mockReturnValue(tempHome)
  process.env.FISH_VERSION = '3.7.0'
  try {
    await expect(setup.handler({ pnpmHomeDir: path.join(actualOs.tmpdir(), `pnpm-home${path.delimiter}evil`) })).rejects.toMatchObject({
      code: 'ERR_PNPM_UNSAFE_SHELL_CONFIG',
    })
    expect(actualFs.existsSync(path.join(tempHome, '.config/fish/conf.d/pnpm.fish'))).toBe(false)
  } finally {
    actualFs.rmSync(tempHome, { force: true, recursive: true })
  }
})

testIfSymlinkSupported('setup refuses to overwrite a symlinked fish config', async () => {
  const tempHome = actualFs.mkdtempSync(path.join(actualOs.tmpdir(), 'pnpm-setup-fish-symlink-'))
  jest.mocked(os.default.homedir).mockReturnValue(tempHome)
  process.env.FISH_VERSION = '3.7.0'
  try {
    const configFile = path.join(tempHome, '.config/fish/conf.d/pnpm.fish')
    const targetFile = path.join(tempHome, 'target.fish')
    actualFs.mkdirSync(path.dirname(configFile), { recursive: true })
    actualFs.writeFileSync(targetFile, 'original')
    actualFs.symlinkSync(targetFile, configFile, 'file')

    await expect(setup.handler({ force: true, pnpmHomeDir: '/pnpm-home' })).rejects.toMatchObject({
      code: 'ERR_PNPM_UNSAFE_SHELL_CONFIG',
    })
    expect(actualFs.readFileSync(targetFile, 'utf8')).toBe('original')
  } finally {
    actualFs.rmSync(tempHome, { force: true, recursive: true })
  }
})

testIfSymlinkSupported('setup refuses to write through a symlinked fish config home', async () => {
  const tempHome = actualFs.mkdtempSync(path.join(actualOs.tmpdir(), 'pnpm-setup-fish-config-home-link-'))
  jest.mocked(os.default.homedir).mockReturnValue(tempHome)
  process.env.FISH_VERSION = '3.7.0'
  try {
    const configHome = path.join(tempHome, '.config')
    const outsideDir = path.join(tempHome, 'outside')
    actualFs.mkdirSync(outsideDir)
    actualFs.symlinkSync(outsideDir, configHome, 'dir')

    await expect(setup.handler({ pnpmHomeDir: '/pnpm-home' })).rejects.toMatchObject({
      code: 'ERR_PNPM_UNSAFE_SHELL_CONFIG',
    })
    expect(actualFs.existsSync(path.join(outsideDir, 'fish/conf.d/pnpm.fish'))).toBe(false)
  } finally {
    actualFs.rmSync(tempHome, { force: true, recursive: true })
  }
})

testIfSymlinkSupported('setup refuses to write through a symlinked fish config directory', async () => {
  const tempHome = actualFs.mkdtempSync(path.join(actualOs.tmpdir(), 'pnpm-setup-fish-dir-link-'))
  jest.mocked(os.default.homedir).mockReturnValue(tempHome)
  process.env.FISH_VERSION = '3.7.0'
  try {
    const configHome = path.join(tempHome, '.config')
    const outsideDir = path.join(tempHome, 'outside')
    actualFs.mkdirSync(configHome)
    actualFs.mkdirSync(outsideDir)
    actualFs.symlinkSync(outsideDir, path.join(configHome, 'fish'), 'dir')

    await expect(setup.handler({ pnpmHomeDir: '/pnpm-home' })).rejects.toMatchObject({
      code: 'ERR_PNPM_UNSAFE_SHELL_CONFIG',
    })
    expect(actualFs.existsSync(path.join(outsideDir, 'conf.d/pnpm.fish'))).toBe(false)
  } finally {
    actualFs.rmSync(tempHome, { force: true, recursive: true })
  }
})

testIfSymlinkSupported('setup refuses to write through a symlinked fish config parent directory', async () => {
  const tempHome = actualFs.mkdtempSync(path.join(actualOs.tmpdir(), 'pnpm-setup-fish-parent-link-'))
  jest.mocked(os.default.homedir).mockReturnValue(tempHome)
  process.env.FISH_VERSION = '3.7.0'
  try {
    const fishDir = path.join(tempHome, '.config/fish')
    const linkedConfDir = path.join(fishDir, 'conf.d')
    const outsideDir = path.join(tempHome, 'outside')
    actualFs.mkdirSync(fishDir, { recursive: true })
    actualFs.mkdirSync(outsideDir)
    actualFs.symlinkSync(outsideDir, linkedConfDir, 'dir')

    await expect(setup.handler({ pnpmHomeDir: '/pnpm-home' })).rejects.toMatchObject({
      code: 'ERR_PNPM_UNSAFE_SHELL_CONFIG',
    })
    expect(actualFs.existsSync(path.join(outsideDir, 'pnpm.fish'))).toBe(false)
  } finally {
    actualFs.rmSync(tempHome, { force: true, recursive: true })
  }
})

test('setup refuses to read a non-regular fish config path', async () => {
  const tempHome = actualFs.mkdtempSync(path.join(actualOs.tmpdir(), 'pnpm-setup-fish-non-regular-'))
  jest.mocked(os.default.homedir).mockReturnValue(tempHome)
  process.env.FISH_VERSION = '3.7.0'
  try {
    const configFile = path.join(tempHome, '.config/fish/conf.d/pnpm.fish')
    actualFs.mkdirSync(configFile, { recursive: true })

    await expect(setup.handler({ pnpmHomeDir: '/pnpm-home' })).rejects.toMatchObject({
      code: 'ERR_PNPM_UNSAFE_SHELL_CONFIG',
    })
  } finally {
    actualFs.rmSync(tempHome, { force: true, recursive: true })
  }
})

test('setup preserves an existing fish config mode when force overwrites it', async () => {
  const tempHome = actualFs.mkdtempSync(path.join(actualOs.tmpdir(), 'pnpm-setup-fish-mode-'))
  jest.mocked(os.default.homedir).mockReturnValue(tempHome)
  process.env.FISH_VERSION = '3.7.0'
  try {
    const configFile = path.join(tempHome, '.config/fish/conf.d/pnpm.fish')
    actualFs.mkdirSync(path.dirname(configFile), { recursive: true })
    actualFs.writeFileSync(configFile, 'old settings\n', { mode: 0o600 })

    await setup.handler({ force: true, pnpmHomeDir: '/pnpm-home' })

    expect(actualFs.statSync(configFile).mode & 0o777).toBe(0o600)
    expect(actualFs.readFileSync(configFile, 'utf8')).toContain('set -gx PNPM_HOME "/pnpm-home"')
  } finally {
    actualFs.rmSync(tempHome, { force: true, recursive: true })
  }
})

test('setup retries fish config overwrite when rename cannot replace destination', async () => {
  const tempHome = actualFs.mkdtempSync(path.join(actualOs.tmpdir(), 'pnpm-setup-fish-rename-retry-'))
  const originalRename = actualFs.promises.rename
  const renameSpy = jest.spyOn(actualFs.promises, 'rename')
  jest.mocked(os.default.homedir).mockReturnValue(tempHome)
  process.env.FISH_VERSION = '3.7.0'
  try {
    const configFile = path.join(tempHome, '.config/fish/conf.d/pnpm.fish')
    actualFs.mkdirSync(path.dirname(configFile), { recursive: true })
    actualFs.writeFileSync(configFile, 'old settings\n', { mode: 0o600 })

    renameSpy
      .mockRejectedValueOnce(Object.assign(new Error('destination exists'), { code: 'EEXIST' }))
      .mockImplementationOnce(originalRename)

    await setup.handler({ force: true, pnpmHomeDir: '/pnpm-home' })

    expect(renameSpy).toHaveBeenCalledTimes(2)
    expect(actualFs.statSync(configFile).mode & 0o777).toBe(0o600)
    expect(actualFs.readFileSync(configFile, 'utf8')).toContain('set -gx PNPM_HOME "/pnpm-home"')
  } finally {
    renameSpy.mockRestore()
    actualFs.rmSync(tempHome, { force: true, recursive: true })
  }
})

test('setup skips existing fish config with CRLF line endings', async () => {
  const tempHome = actualFs.mkdtempSync(path.join(actualOs.tmpdir(), 'pnpm-setup-fish-'))
  jest.mocked(os.default.homedir).mockReturnValue(tempHome)
  process.env.FISH_VERSION = '3.7.0'
  try {
    const configFile = path.join(tempHome, '.config/fish/conf.d/pnpm.fish')
    actualFs.mkdirSync(path.dirname(configFile), { recursive: true })
    actualFs.writeFileSync(configFile, [
      'set -gx PNPM_HOME "/pnpm-home"',
      'if not string match -q -- "$PNPM_HOME/bin" $PATH',
      '  set -gx PATH "$PNPM_HOME/bin" $PATH',
      'end',
      '',
    ].join('\r\n'))

    const output = await setup.handler({ pnpmHomeDir: '/pnpm-home' })

    expect(actualFs.readFileSync(configFile, 'utf8')).toContain('\r\n')
    expect(jest.mocked(addDirToEnvPath)).not.toHaveBeenCalled()
    expect(output).toBe('No changes to the environment were made. Everything is already up to date.')
  } finally {
    actualFs.rmSync(tempHome, { force: true, recursive: true })
  }
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
  const tmpDir = actualFs.mkdtempSync(path.join(actualOs.tmpdir(), 'pnpm-setup-test-'))
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
