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

test('Environment PATH is not configured correctly', async () => {
  execa['mockResolvedValueOnce']({
    failed: false,
    stdout: '活动代码页: 936',
  }).mockResolvedValueOnce({
    failed: false,
    stdout: '',
  }).mockResolvedValueOnce({
    failed: false,
    stdout: 'SOME KIND OF ERROR OR UNSUPPORTED RESPONSE FORMAT',
  }).mockResolvedValue({
    failed: true,
  })

  const output = await setup.handler({
    pnpmHomeDir: __dirname,
  })

  expect(execa).toHaveBeenNthCalledWith(3, 'reg', ['query', regKey], { windowsHide: false })
  expect(output).toContain('Current PATH is not set. No changes to this environment variable are applied')
})
