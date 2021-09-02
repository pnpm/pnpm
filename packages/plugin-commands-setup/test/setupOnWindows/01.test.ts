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

test('Win32 registry environment values could not be retrieved', async () => {
  execa['mockResolvedValue']({
    failed: true,
  })

  const output = await setup.handler({
    pnpmHomeDir: __dirname,
  })

  expect(execa).toHaveBeenNthCalledWith(1, 'reg', ['query', regKey])
  expect(output).toContain('Win32 registry environment values could not be retrieved')
})
