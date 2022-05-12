import { win32 as path } from 'path'
import execa from 'execa'
import { setup } from '@pnpm/plugin-commands-setup'
import { tempDir } from '@pnpm/prepare'

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

test('Failure to install', async () => {
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
    failed: true,
    stderr: 'FAILED TO SET PNPM_HOME',
  }).mockResolvedValueOnce({
    failed: true,
    stderr: 'FAILED TO UPDATE PATH',
  }).mockResolvedValue({
    failed: true,
    stderr: 'UNEXPECTED',
  })

  const pnpmHomeDir = tempDir(false)
  const pnpmHomeDirNormalized = path.normalize(pnpmHomeDir)
  const output = await setup.handler({ pnpmHomeDir })

  expect(execa).toHaveBeenNthCalledWith(3, 'reg', ['query', regKey], { windowsHide: false })
  expect(execa).toHaveBeenNthCalledWith(4, 'reg', ['add', regKey, '/v', 'PNPM_HOME', '/t', 'REG_EXPAND_SZ', '/d', pnpmHomeDirNormalized, '/f'], { windowsHide: false })
  expect(execa).toHaveBeenNthCalledWith(5, 'reg', ['add', regKey, '/v', 'Path', '/t', 'REG_EXPAND_SZ', '/d', `%PNPM_HOME%;${currentPathInRegistry}`, '/f'], { windowsHide: false })
  expect(output).toContain(`Setting 'PNPM_HOME' to value '${pnpmHomeDirNormalized}`)
  expect(output).toContain('FAILED TO SET PNPM_HOME')
  expect(output).toContain('Updating PATH')
  expect(output).toContain('FAILED TO UPDATE PATH')
})
