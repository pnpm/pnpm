import { win32 as path } from 'path'
import execa from 'execa'
import { setup } from '@pnpm/plugin-commands-setup'
import { tempDir } from '@pnpm/prepare'

jest.mock('execa')

let originalShell: string | undefined
let originalPlatform = ''

beforeEach(() => {
  execa['mockReset']()
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
  const err = new Error('Command failed with exit code 1')
  err['stderr'] = stderr
  return err
}

test('win32 registry environment values could not be retrieved', async () => {
  execa['mockResolvedValueOnce']({
    failed: false,
    stdout: '活动代码页: 936',
  }).mockResolvedValueOnce({
    failed: false,
    stdout: '',
  }).mockRejectedValue(createExecaError({ stderr: 'ERROR: Some error' }))

  await expect(setup.handler({ pnpmHomeDir: tempDir(false) }))
    .rejects.toThrow()
})

test('environment Path is not configured correctly', async () => {
  execa['mockResolvedValueOnce']({
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
    setup.handler({
      pnpmHomeDir: tempDir(false),
    })
  ).rejects.toThrow(/"Path" environment variable is not found/)

  expect(execa).toHaveBeenNthCalledWith(3, 'reg', ['query', regKey], { windowsHide: false })
})

test('environment Path is empty', async () => {
  execa['mockResolvedValueOnce']({
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
    setup.handler({
      pnpmHomeDir: tempDir(false),
    })
  ).rejects.toThrow(/"Path" environment variable is not found/)

  expect(execa).toHaveBeenNthCalledWith(3, 'reg', ['query', regKey], { windowsHide: false })
})

test('successful first time installation', async () => {
  const currentPathInRegistry = '%USERPROFILE%\\AppData\\Local\\Microsoft\\WindowsApps;%USERPROFILE%\\.config\\etc;'

  execa['mockResolvedValueOnce']({
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
  const output = await setup.handler({ pnpmHomeDir })

  expect(execa).toHaveBeenNthCalledWith(3, 'reg', ['query', regKey], { windowsHide: false })
  expect(execa).toHaveBeenNthCalledWith(4, 'reg', ['add', regKey, '/v', 'PNPM_HOME', '/t', 'REG_EXPAND_SZ', '/d', pnpmHomeDirNormalized, '/f'], { windowsHide: false })
  expect(execa).toHaveBeenNthCalledWith(5, 'setx', ['PNPM_HOME', pnpmHomeDirNormalized], { windowsHide: false })
  expect(execa).toHaveBeenNthCalledWith(6, 'reg', ['add', regKey, '/v', 'Path', '/t', 'REG_EXPAND_SZ', '/d', `%PNPM_HOME%;${currentPathInRegistry}`, '/f'], { windowsHide: false })
  expect(execa).toHaveBeenNthCalledWith(7, 'setx', ['Path', `%PNPM_HOME%;${currentPathInRegistry}`], { windowsHide: false })
  expect(output).toContain('Path was updated')
  expect(output).toContain('PNPM_HOME was updated')
})

test('PNPM_HOME is already set, but Path is updated', async () => {
  const currentPathInRegistry = '%USERPROFILE%\\AppData\\Local\\Microsoft\\WindowsApps;%USERPROFILE%\\.config\\etc;'
  const pnpmHomeDir = tempDir(false)
  const pnpmHomeDirNormalized = path.normalize(pnpmHomeDir)
  execa['mockResolvedValueOnce']({
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

  const output = await setup.handler({ pnpmHomeDir })

  expect(execa).toHaveBeenNthCalledWith(3, 'reg', ['query', regKey], { windowsHide: false })
  expect(execa).toHaveBeenNthCalledWith(4, 'reg', ['add', regKey, '/v', 'Path', '/t', 'REG_EXPAND_SZ', '/d', `%PNPM_HOME%;${currentPathInRegistry}`, '/f'], { windowsHide: false })
  expect(execa).toHaveBeenNthCalledWith(5, 'setx', ['Path', `%PNPM_HOME%;${currentPathInRegistry}`], { windowsHide: false })
  expect(output).toContain('PNPM_HOME was already up-to-date')
  expect(output).toContain('Path was updated')
})

test('setup throws an error if PNPM_HOME is already set to a different directory', async () => {
  execa['mockResolvedValueOnce']({
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
    setup.handler({ pnpmHomeDir })
  ).rejects.toThrowError(/Currently 'PNPM_HOME' is set to/)
})

test('setup overrides PNPM_HOME, when setup is forced', async () => {
  execa['mockReset']()
  execa['mockResolvedValueOnce']({
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
  const output = await setup.handler({
    force: true,
    pnpmHomeDir,
  })

  expect(execa).toHaveBeenNthCalledWith(3, 'reg', ['query', regKey], { windowsHide: false })
  expect(execa).toHaveBeenNthCalledWith(4, 'reg', ['add', regKey, '/v', 'PNPM_HOME', '/t', 'REG_EXPAND_SZ', '/d', pnpmHomeDirNormalized, '/f'], { windowsHide: false })
  expect(output).toContain('PNPM_HOME was updated')
})

test('failure to install', async () => {
  const currentPathInRegistry = '%USERPROFILE%\\AppData\\Local\\Microsoft\\WindowsApps;%USERPROFILE%\\.config\\etc;'

  execa['mockResolvedValueOnce']({
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
    setup.handler({ pnpmHomeDir })
  ).rejects.toThrow()
})
