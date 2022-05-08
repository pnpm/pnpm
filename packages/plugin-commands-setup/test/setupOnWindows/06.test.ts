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

test('Existing installation', async () => {
  execa['mockResolvedValueOnce']({
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

  const output = await setup.handler({
    pnpmHomeDir: __dirname,
  })

  expect(execa).toHaveBeenNthCalledWith(1, `chcp 65001>nul && reg query ${regKey}`, undefined, { shell: true })
  expect(output).toContain(`Currently 'PNPM_HOME' is set to '${'.pnpm\\home'}'`)
  expect(output).toContain('PATH already contains PNPM_HOME')
})
