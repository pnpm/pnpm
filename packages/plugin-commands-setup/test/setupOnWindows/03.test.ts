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

test('environment PATH is empty', async () => {
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
  ).rejects.toThrow(/PATH environment variable is not found/)

  expect(execa).toHaveBeenNthCalledWith(3, 'reg', ['query', regKey, '/v', 'PNPM_HOME'], { windowsHide: false })
})
