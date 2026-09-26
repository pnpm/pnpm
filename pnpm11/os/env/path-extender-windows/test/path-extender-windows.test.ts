import { win32 as path } from 'node:path'

import { afterAll, beforeAll, beforeEach, expect, jest, test } from '@jest/globals'
import { tempDir } from '@pnpm/prepare'

const execaMock = jest.fn<(...args: unknown[]) => Promise<unknown>>()
jest.unstable_mockModule('safe-execa', () => ({ safeExeca: execaMock }))

const { addDirToWindowsEnvPath } = await import('@pnpm/os.env.path-extender-windows')

let originalShell: string | undefined
let originalPlatform = ''

beforeEach(() => {
  execaMock.mockReset()
})

beforeAll(() => {
  originalShell = process.env.SHELL
  originalPlatform = process.platform

  process.env.SHELL = ''
  Object.defineProperty(process, 'platform', {
    value: 'win32',
  })
})

afterAll(() => {
  process.env.SHELL = originalShell
  Object.defineProperty(process, 'platform', {
    value: originalPlatform,
  })
})

const regKey = 'HKEY_CURRENT_USER\\Environment'

function createExecaError ({ stderr }: { stderr: string}) {
  return Object.assign(new Error('Command failed with exit code 1'), { stderr })
}

test('win32 registry environment values could not be retrieved', async () => {
  execaMock.mockResolvedValueOnce({
    failed: false,
    stdout: '活动代码页: 936',
  }).mockResolvedValueOnce({
    failed: false,
    stdout: '',
  }).mockRejectedValue(createExecaError({ stderr: 'ERROR: Some error' }))

  await expect(addDirToWindowsEnvPath(tempDir(false), { proxyVarName: 'PNPM_HOME' }))
    .rejects.toThrow()
})

test('environment Path is not configured correctly', async () => {
  execaMock.mockResolvedValueOnce({
    failed: false,
    stdout: '活动代码页: 936',
  }).mockResolvedValueOnce({
    failed: false,
    stdout: 'SOME KIND OF ERROR OR UNSUPPORTED RESPONSE FORMAT',
  }).mockResolvedValue({
    failed: false,
    stdout: '',
  })

  await expect(
    addDirToWindowsEnvPath(tempDir(false), { proxyVarName: 'PNPM_HOME' })
  ).rejects.toThrow(/"Path" environment variable is not found/)

  expect(execaMock).toHaveBeenNthCalledWith(3, 'reg', ['query', regKey], { windowsHide: false })
})

test('environment Path is empty', async () => {
  execaMock.mockResolvedValueOnce({
    failed: false,
    stdout: '活动代码页: 936',
  }).mockResolvedValueOnce({
    failed: false,
    stdout: '',
  }).mockResolvedValueOnce({
    failed: false,
    stdout: `
HKEY_CURRENT_USER\\Environment
    Path    REG_EXPAND_SZ
    `,
  }).mockResolvedValue({
    failed: false,
    stdout: '',
  })

  await expect(
    addDirToWindowsEnvPath(tempDir(false), { proxyVarName: 'PNPM_HOME' })
  ).rejects.toThrow(/"Path" environment variable is not found/)

  expect(execaMock).toHaveBeenNthCalledWith(3, 'reg', ['query', regKey], { windowsHide: false })
})

test('successful first time installation', async () => {
  const currentPathInRegistry = '%USERPROFILE%\\AppData\\Local\\Microsoft\\WindowsApps;%USERPROFILE%\\.config\\etc;'

  execaMock.mockResolvedValueOnce({
    failed: false,
    stdout: '活动代码页: 936',
  }).mockResolvedValueOnce({
    failed: false,
    stdout: '',
  }).mockResolvedValueOnce({
    failed: false,
    stdout: `
HKEY_CURRENT_USER\\Environment
    Path    REG_EXPAND_SZ    ${currentPathInRegistry}
`,
  }).mockResolvedValueOnce({
    failed: false,
    stdout: 'PNPM_HOME ENV VAR SET',
  }).mockResolvedValueOnce({
    failed: false,
    stdout: 'setx PNPM_HOME',
  }).mockResolvedValueOnce({
    failed: false,
    stdout: 'setx PNPM_HOME',
  }).mockResolvedValue({
    failed: true,
    stderr: 'UNEXPECTED',
  })

  const pnpmHomeDir = tempDir(false)
  const pnpmHomeDirNormalized = path.normalize(pnpmHomeDir)
  const report = await addDirToWindowsEnvPath(pnpmHomeDir, { proxyVarName: 'PNPM_HOME' })

  expect(report).toStrictEqual([
    {
      action: 'updated',
      variable: 'PNPM_HOME',
      oldValue: undefined,
      newValue: pnpmHomeDirNormalized,
    },
    {
      action: 'updated',
      variable: 'Path',
      oldValue: currentPathInRegistry,
      newValue: `%PNPM_HOME%;${currentPathInRegistry}`,
    },
  ])
  expect(execaMock).toHaveBeenNthCalledWith(3, 'reg', ['query', regKey], { windowsHide: false })
  expect(execaMock).toHaveBeenNthCalledWith(4, 'reg', ['add', regKey, '/v', 'PNPM_HOME', '/t', 'REG_SZ', '/d', pnpmHomeDirNormalized, '/f'], { windowsHide: false })
  expect(execaMock).toHaveBeenNthCalledWith(5, 'reg', ['add', regKey, '/v', 'Path', '/t', 'REG_EXPAND_SZ', '/d', `%PNPM_HOME%;${currentPathInRegistry}`, '/f'], { windowsHide: false })
  expect(execaMock).toHaveBeenNthCalledWith(6, 'setx', ['REFRESH_ENV_VARS', '1'], { windowsHide: false })
  expect(execaMock).toHaveBeenNthCalledWith(7, 'reg', ['delete', regKey, '/v', 'REFRESH_ENV_VARS', '/f'], { windowsHide: false })
})

test('successful first time installation with proxyVarSubDir', async () => {
  const currentPathInRegistry = '%USERPROFILE%\\AppData\\Local\\Microsoft\\WindowsApps;%USERPROFILE%\\.config\\etc;'

  execaMock.mockResolvedValueOnce({
    failed: false,
    stdout: '活动代码页: 936',
  }).mockResolvedValueOnce({
    failed: false,
    stdout: '',
  }).mockResolvedValueOnce({
    failed: false,
    stdout: `
HKEY_CURRENT_USER\\Environment
    Path    REG_EXPAND_SZ    ${currentPathInRegistry}
`,
  }).mockResolvedValueOnce({
    failed: false,
    stdout: 'PNPM_HOME ENV VAR SET',
  }).mockResolvedValueOnce({
    failed: false,
    stdout: 'setx PNPM_HOME',
  }).mockResolvedValueOnce({
    failed: false,
    stdout: 'setx PNPM_HOME',
  }).mockResolvedValue({
    failed: true,
    stderr: 'UNEXPECTED',
  })

  const pnpmHomeDir = tempDir(false)
  const pnpmHomeDirNormalized = path.normalize(pnpmHomeDir)
  const report = await addDirToWindowsEnvPath(pnpmHomeDir, { proxyVarName: 'PNPM_HOME', proxyVarSubDir: 'bin' })

  expect(report).toStrictEqual([
    {
      action: 'updated',
      variable: 'PNPM_HOME',
      oldValue: undefined,
      newValue: pnpmHomeDirNormalized,
    },
    {
      action: 'updated',
      variable: 'Path',
      oldValue: currentPathInRegistry,
      newValue: `%PNPM_HOME%\\bin;${currentPathInRegistry}`,
    },
  ])
  expect(execaMock).toHaveBeenNthCalledWith(5, 'reg', ['add', regKey, '/v', 'Path', '/t', 'REG_EXPAND_SZ', '/d', `%PNPM_HOME%\\bin;${currentPathInRegistry}`, '/f'], { windowsHide: false })
})

test('successful first time installation when no additional env variable is used', async () => {
  const currentPathInRegistry = '%USERPROFILE%\\AppData\\Local\\Microsoft\\WindowsApps;%USERPROFILE%\\.config\\etc;'

  execaMock.mockResolvedValueOnce({
    failed: false,
    stdout: '活动代码页: 936',
  }).mockResolvedValueOnce({
    failed: false,
    stdout: '',
  }).mockResolvedValueOnce({
    failed: false,
    stdout: `
HKEY_CURRENT_USER\\Environment
    Path    REG_EXPAND_SZ    ${currentPathInRegistry}
`,
  }).mockResolvedValueOnce({
    failed: false,
    stdout: 'PNPM_HOME ENV VAR SET',
  }).mockResolvedValueOnce({
    failed: false,
    stdout: 'setx PNPM_HOME',
  }).mockResolvedValueOnce({
    failed: false,
    stdout: 'setx PNPM_HOME',
  }).mockResolvedValue({
    failed: true,
    stderr: 'UNEXPECTED',
  })

  const pnpmHomeDir = tempDir(false)
  const pnpmHomeDirNormalized = path.normalize(pnpmHomeDir)
  const report = await addDirToWindowsEnvPath(pnpmHomeDir)

  expect(report).toStrictEqual([
    {
      action: 'updated',
      variable: 'Path',
      oldValue: currentPathInRegistry,
      newValue: `${pnpmHomeDirNormalized};${currentPathInRegistry}`,
    },
  ])
  expect(execaMock).toHaveBeenNthCalledWith(3, 'reg', ['query', regKey], { windowsHide: false })
  expect(execaMock).toHaveBeenNthCalledWith(4, 'reg', ['add', regKey, '/v', 'Path', '/t', 'REG_EXPAND_SZ', '/d', `${pnpmHomeDirNormalized};${currentPathInRegistry}`, '/f'], { windowsHide: false })
  expect(execaMock).toHaveBeenNthCalledWith(5, 'setx', ['REFRESH_ENV_VARS', '1'], { windowsHide: false })
  expect(execaMock).toHaveBeenNthCalledWith(6, 'reg', ['delete', regKey, '/v', 'REFRESH_ENV_VARS', '/f'], { windowsHide: false })
})

test('adding the directory to the end of Path', async () => {
  const currentPathInRegistry = '%USERPROFILE%\\AppData\\Local\\Microsoft\\WindowsApps;%USERPROFILE%\\.config\\etc;'

  execaMock.mockResolvedValueOnce({
    failed: false,
    stdout: '活动代码页: 936',
  }).mockResolvedValueOnce({
    failed: false,
    stdout: '',
  }).mockResolvedValueOnce({
    failed: false,
    stdout: `
HKEY_CURRENT_USER\\Environment
    Path    REG_EXPAND_SZ    ${currentPathInRegistry}
`,
  }).mockResolvedValueOnce({
    failed: false,
    stdout: 'PNPM_HOME ENV VAR SET',
  }).mockResolvedValueOnce({
    failed: false,
    stdout: 'setx PNPM_HOME',
  }).mockResolvedValueOnce({
    failed: false,
    stdout: 'setx PNPM_HOME',
  }).mockResolvedValue({
    failed: true,
    stderr: 'UNEXPECTED',
  })

  const pnpmHomeDir = tempDir(false)
  const pnpmHomeDirNormalized = path.normalize(pnpmHomeDir)
  const report = await addDirToWindowsEnvPath(pnpmHomeDir, { position: 'end' })

  expect(report).toStrictEqual([
    {
      action: 'updated',
      variable: 'Path',
      oldValue: currentPathInRegistry,
      newValue: `${currentPathInRegistry};${pnpmHomeDirNormalized}`,
    },
  ])
  expect(execaMock).toHaveBeenNthCalledWith(3, 'reg', ['query', regKey], { windowsHide: false })
  expect(execaMock).toHaveBeenNthCalledWith(4, 'reg', ['add', regKey, '/v', 'Path', '/t', 'REG_EXPAND_SZ', '/d', `${currentPathInRegistry};${pnpmHomeDirNormalized}`, '/f'], { windowsHide: false })
  expect(execaMock).toHaveBeenNthCalledWith(5, 'setx', ['REFRESH_ENV_VARS', '1'], { windowsHide: false })
  expect(execaMock).toHaveBeenNthCalledWith(6, 'reg', ['delete', regKey, '/v', 'REFRESH_ENV_VARS', '/f'], { windowsHide: false })
})

test('PNPM_HOME is already set, but Path is updated', async () => {
  const currentPathInRegistry = '%USERPROFILE%\\AppData\\Local\\Microsoft\\WindowsApps;%USERPROFILE%\\.config\\etc;'
  const pnpmHomeDir = tempDir(false)
  const pnpmHomeDirNormalized = path.normalize(pnpmHomeDir)
  execaMock.mockResolvedValueOnce({
    failed: false,
    stdout: '活动代码页: 936',
  }).mockResolvedValueOnce({
    failed: false,
    stdout: '',
  }).mockResolvedValueOnce({
    failed: false,
    stdout: `
HKEY_CURRENT_USER\\Environment
    PNPM_HOME    REG_EXPAND_SZ    ${pnpmHomeDirNormalized}
    Path    REG_EXPAND_SZ    ${currentPathInRegistry}
`,
  }).mockResolvedValueOnce({
    failed: false,
    stdout: 'Path UPDATED',
  }).mockResolvedValueOnce({
    failed: false,
    stdout: 'setx PATH',
  }).mockResolvedValue({
    failed: true,
    stderr: 'UNEXPECTED',
  })

  const report = await addDirToWindowsEnvPath(pnpmHomeDir, { proxyVarName: 'PNPM_HOME' })

  expect(report).toStrictEqual([
    {
      variable: 'PNPM_HOME',
      action: 'updated',
      oldValue: pnpmHomeDirNormalized,
      newValue: pnpmHomeDirNormalized,
    },
    {
      variable: 'Path',
      action: 'updated',
      oldValue: currentPathInRegistry,
      newValue: `%PNPM_HOME%;${currentPathInRegistry}`,
    },
  ])
  expect(execaMock).toHaveBeenNthCalledWith(3, 'reg', ['query', regKey], { windowsHide: false })
  expect(execaMock).toHaveBeenNthCalledWith(4, 'reg', ['add', regKey, '/v', 'PNPM_HOME', '/t', 'REG_SZ', '/d', pnpmHomeDirNormalized, '/f'], { windowsHide: false })
  expect(execaMock).toHaveBeenNthCalledWith(5, 'reg', ['add', regKey, '/v', 'Path', '/t', 'REG_EXPAND_SZ', '/d', `%PNPM_HOME%;${currentPathInRegistry}`, '/f'], { windowsHide: false })
  expect(execaMock).toHaveBeenNthCalledWith(6, 'setx', ['REFRESH_ENV_VARS', '1'], { windowsHide: false })
  expect(execaMock).toHaveBeenNthCalledWith(7, 'reg', ['delete', regKey, '/v', 'REFRESH_ENV_VARS', '/f'], { windowsHide: false })
})

test('PNPM_HOME with the correct registry type is left unchanged', async () => {
  const pnpmHomeDir = tempDir(false)
  const pnpmHomeDirNormalized = path.normalize(pnpmHomeDir)
  const currentPathInRegistry = '%PNPM_HOME%;C:\\Windows'
  execaMock.mockResolvedValueOnce({
    failed: false,
    stdout: 'Active code page: 437',
  }).mockResolvedValueOnce({
    failed: false,
    stdout: '',
  }).mockResolvedValueOnce({
    failed: false,
    stdout: '\nHKEY_CURRENT_USER\\Environment\n    PNPM_HOME    REG_SZ    ' + pnpmHomeDirNormalized + '\n    Path    REG_EXPAND_SZ    ' + currentPathInRegistry + '\n',
  }).mockResolvedValueOnce({
    failed: false,
    stdout: '',
  }).mockResolvedValueOnce({
    failed: false,
    stdout: '',
  }).mockResolvedValueOnce({
    failed: false,
    stdout: '',
  })

  const report = await addDirToWindowsEnvPath(pnpmHomeDir, { proxyVarName: 'PNPM_HOME' })

  expect(report).toStrictEqual([
    {
      variable: 'PNPM_HOME',
      action: 'skipped',
      oldValue: pnpmHomeDirNormalized,
      newValue: pnpmHomeDirNormalized,
    },
    {
      variable: 'Path',
      action: 'skipped',
      oldValue: currentPathInRegistry,
      newValue: currentPathInRegistry,
    },
  ])
  expect(execaMock).not.toHaveBeenCalledWith('reg', expect.arrayContaining(['add']), { windowsHide: false })
})

test('setup throws an error if PNPM_HOME is already set to a different directory', async () => {
  execaMock.mockResolvedValueOnce({
    failed: false,
    stdout: '活动代码页: 936',
  }).mockResolvedValueOnce({
    failed: false,
    stdout: '',
  }).mockResolvedValueOnce({
    failed: false,
    stdout: `
HKEY_CURRENT_USER\\Environment
    PNPM_HOME    REG_EXPAND_SZ    .pnpm\\home
    Path    REG_EXPAND_SZ    %USERPROFILE%\\AppData\\Local\\Microsoft\\WindowsApps;%USERPROFILE%\\.config\\etc;.pnpm\\home;C:\\Windows;
`,
  }).mockResolvedValue({
    failed: true,
    stderr: 'UNEXPECTED',
  })

  const pnpmHomeDir = tempDir(false)
  await expect(
    addDirToWindowsEnvPath(pnpmHomeDir, { proxyVarName: 'PNPM_HOME' })
  ).rejects.toThrow(/Currently 'PNPM_HOME' is set to/)
})

test('setup overwrites PNPM_HOME, when setup is forced', async () => {
  execaMock.mockReset()
  execaMock.mockResolvedValueOnce({
    failed: false,
    stdout: '活动代码页: 936',
  }).mockResolvedValueOnce({
    failed: false,
    stdout: '',
  }).mockResolvedValueOnce({
    failed: false,
    stdout: `
HKEY_CURRENT_USER\\Environment
    PNPM_HOME    REG_EXPAND_SZ    .pnpm\\home
    Path    REG_EXPAND_SZ    %USERPROFILE%\\AppData\\Local\\Microsoft\\WindowsApps;%USERPROFILE%\\.config\\etc;.pnpm\\home;C:\\Windows;
`,
  }).mockResolvedValueOnce({
    failed: false,
    stdout: '',
  }).mockResolvedValueOnce({
    failed: false,
    stdout: '',
  }).mockResolvedValueOnce({
  }).mockResolvedValueOnce({
    failed: false,
    stdout: '',
  }).mockResolvedValueOnce({
    failed: false,
    stdout: '',
  }).mockResolvedValue({
    failed: true,
    stderr: 'UNEXPECTED',
  })

  const pnpmHomeDir = tempDir(false)
  const pnpmHomeDirNormalized = path.normalize(pnpmHomeDir)
  const report = await addDirToWindowsEnvPath(pnpmHomeDir, {
    proxyVarName: 'PNPM_HOME',
    overwriteProxyVar: true,
  })

  expect(report).toStrictEqual([
    {
      variable: 'PNPM_HOME',
      action: 'updated',
      oldValue: '.pnpm\\home',
      newValue: pnpmHomeDirNormalized,
    },
    {
      variable: 'Path',
      action: 'updated',
      oldValue: '%USERPROFILE%\\AppData\\Local\\Microsoft\\WindowsApps;%USERPROFILE%\\.config\\etc;.pnpm\\home;C:\\Windows;',
      newValue: '%PNPM_HOME%;%USERPROFILE%\\AppData\\Local\\Microsoft\\WindowsApps;%USERPROFILE%\\.config\\etc;.pnpm\\home;C:\\Windows;',
    },
  ])
  expect(execaMock).toHaveBeenNthCalledWith(3, 'reg', ['query', regKey], { windowsHide: false })
  expect(execaMock).toHaveBeenNthCalledWith(4, 'reg', ['add', regKey, '/v', 'PNPM_HOME', '/t', 'REG_SZ', '/d', pnpmHomeDirNormalized, '/f'], { windowsHide: false })
})

test('failure to install', async () => {
  const currentPathInRegistry = '%USERPROFILE%\\AppData\\Local\\Microsoft\\WindowsApps;%USERPROFILE%\\.config\\etc;'

  execaMock.mockResolvedValueOnce({
    failed: false,
    stdout: '活动代码页: 936',
  }).mockResolvedValueOnce({
    failed: false,
    stdout: '',
  }).mockResolvedValueOnce({
    failed: false,
    stdout: `
HKEY_CURRENT_USER\\Environment
    Path    REG_EXPAND_SZ    ${currentPathInRegistry}
`,
  }).mockRejectedValue(new Error())

  const pnpmHomeDir = tempDir(false)
  await expect(
    addDirToWindowsEnvPath(pnpmHomeDir)
  ).rejects.toThrow()
})
