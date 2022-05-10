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

test('PNPM_HOME is already set, but path is updated', async () => {
  const currentPathInRegistry = '%USERPROFILE%\\AppData\\Local\\Microsoft\\WindowsApps;%USERPROFILE%\\.config\\etc;'
  const pnpmHomeDir = '.pnpm\\home'
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
    PNPM_HOME    REG_EXPAND_SZ    ${pnpmHomeDir}
    Path    REG_EXPAND_SZ    ${currentPathInRegistry}
`,
  }).mockResolvedValueOnce({
    failed: false,
    stdout: 'PATH UPDATED',
  }).mockResolvedValue({
    failed: true,
    stderr: 'UNEXPECTED',
  })

  const output = await setup.handler({ pnpmHomeDir })

  expect(execa).toHaveBeenNthCalledWith(3, 'reg', ['query', regKey], { windowsHide: false })
  expect(execa).toHaveBeenNthCalledWith(4, 'reg', ['add', regKey, '/v', 'Path', '/t', 'REG_EXPAND_SZ', '/d', `${'.pnpm\\home'};${currentPathInRegistry}`, '/f'], { windowsHide: false })
  expect(execa).toHaveBeenNthCalledWith(5, 'setx', ['PNPM_HOME', '.pnpm\\home'])
  expect(output).toContain('Updating PATH')
  expect(output).toContain('PATH UPDATED')
})
