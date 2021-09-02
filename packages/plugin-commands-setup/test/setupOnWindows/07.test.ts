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

test('Failure to install', async () => {
  const currentPathInRegistry = '%USERPROFILE%\\AppData\\Local\\Microsoft\\WindowsApps;%USERPROFILE%\\.config\\etc;'

  execa['mockResolvedValueOnce']({
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

  const output = await setup.handler({
    pnpmHomeDir: __dirname,
  })

  expect(execa).toHaveBeenNthCalledWith(1, 'reg', ['query', regKey])
  expect(execa).toHaveBeenNthCalledWith(2, 'reg', ['add', regKey, '/v', 'PNPM_HOME', '/t', 'REG_EXPAND_SZ', '/d', __dirname, '/f'])
  expect(execa).toHaveBeenNthCalledWith(3, 'reg', ['add', regKey, '/v', 'Path', '/t', 'REG_EXPAND_SZ', '/d', `${__dirname};${currentPathInRegistry}`, '/f'])
  expect(output).toContain(`Setting 'PNPM_HOME' to value '${__dirname}`)
  expect(output).toContain('FAILED TO SET PNPM_HOME')
  expect(output).toContain('Updating PATH')
  expect(output).toContain('FAILED TO UPDATE PATH')
})