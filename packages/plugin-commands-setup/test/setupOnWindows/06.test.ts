import execa from 'execa'
import { setup } from '@pnpm/plugin-commands-setup'

jest.mock('execa')

let originalShell: string | undefined
let originalPlatform = ''

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

  await expect(
    setup.handler({
      pnpmHomeDir: __dirname,
    })
  ).rejects.toThrowError(/Currently 'PNPM_HOME' is set to '.pnpm\\home'/)
})

test('setup overrides PNPM_HOME, when setup is forced', async () => {
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

  const pnpmHomeDir = '.pnpm\\home'
  const output = await setup.handler({
    force: true,
    pnpmHomeDir,
  })

  expect(execa).toHaveBeenNthCalledWith(3, 'reg', ['query', regKey], { windowsHide: false })
  expect(output).toContain(`Setting 'PNPM_HOME' to value '${pnpmHomeDir}'`)
})
