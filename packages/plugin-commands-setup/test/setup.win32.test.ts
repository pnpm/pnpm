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

test('Win32 registry environment values could not be retrieved', async () => {
  execa['mockResolvedValue']({
    failed: true,
  })

  const output = await setup.handler({
    pnpmHomeDir: __dirname,
  })

  expect(output).toContain('Win32 registry environment values could not be retrieved')
})

test('Environment PATH is not configured correctly', async () => {
  execa['mockResolvedValueOnce']({
    failed: false,
    stdout: 'SOME KIND OF ERROR OR UNSUPPORTED RESPONSE FORMAT',
  }).mockResolvedValue({
    failed: true,
  })

  const output = await setup.handler({
    pnpmHomeDir: __dirname,
  })

  expect(output).toContain('Current PATH is not set. No changes to this environment variable are applied')
})

test('Environment PATH is empty', async () => {
  execa['mockResolvedValueOnce']({
    failed: false,
    stdout: `
HKEY_CURRENT_USER\\Environment
    Path    REG_EXPAND_SZ    
`,
  }).mockResolvedValue({
    failed: false,
  })

  const output = await setup.handler({
    pnpmHomeDir: __dirname,
  })

  expect(output).toContain('Current PATH is empty. No changes to this environment variable are applied')
})

test('Successful first time installation', async () => {
  execa['mockResolvedValueOnce']({
    failed: false,
    stdout: `
HKEY_CURRENT_USER\\Environment
    Path    REG_EXPAND_SZ    %USERPROFILE%\\AppData\\Local\\Microsoft\\WindowsApps;%USERPROFILE%\\.config\\etc;
`,
  }).mockResolvedValueOnce({
    failed: false,
    stdout: 'PNPM_HOME ENV VAR SET',
  }).mockResolvedValueOnce({
    failed: false,
    stdout: 'PATH UPDATED',
  }).mockResolvedValue({
    failed: true,
    stderr: 'UNEXPECTED',
  })

  const output = await setup.handler({
    pnpmHomeDir: __dirname,
  })

  expect(output).toContain(`Setting 'PNPM_HOME' to value '${__dirname}`)
  expect(output).toContain('Updating PATH')
  expect(output).toContain('PNPM_HOME ENV VAR SET')
  expect(output).toContain('PATH UPDATED')
})

test('PNPM_HOME is already set, but path is updated', async () => {
  execa['mockResolvedValueOnce']({
    failed: false,
    stdout: `
HKEY_CURRENT_USER\\Environment
    PNPM_HOME    REG_EXPAND_SZ    .pnpm\\home
    Path    REG_EXPAND_SZ    %USERPROFILE%\\AppData\\Local\\Microsoft\\WindowsApps;%USERPROFILE%\\.config\\etc;
`,
  }).mockResolvedValueOnce({
    failed: false,
    stdout: 'PATH UPDATED',
  }).mockResolvedValue({
    failed: true,
    stderr: 'UNEXPECTED',
  })

  const output = await setup.handler({
    pnpmHomeDir: __dirname,
  })

  expect(output).toContain(`Currently 'PNPM_HOME' is set to '${'.pnpm\\home'}'`)
  expect(output).toContain('Updating PATH')
  expect(output).toContain('PATH UPDATED')
})

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

  expect(output).toContain(`Currently 'PNPM_HOME' is set to '${'.pnpm\\home'}'`)
  expect(output).toContain('PATH already contains PNPM_HOME')
})

test('Failure to install', async () => {
  execa['mockResolvedValueOnce']({
    failed: false,
    stdout: `
HKEY_CURRENT_USER\\Environment
    Path    REG_EXPAND_SZ    %USERPROFILE%\\AppData\\Local\\Microsoft\\WindowsApps;%USERPROFILE%\\.config\\etc;
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

  expect(output).toContain(`Setting 'PNPM_HOME' to value '${__dirname}`)
  expect(output).toContain('FAILED TO SET PNPM_HOME')
  expect(output).toContain('Updating PATH')
  expect(output).toContain('FAILED TO UPDATE PATH')
})