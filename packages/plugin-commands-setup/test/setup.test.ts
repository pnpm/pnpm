import fs from 'fs'

import { homedir } from 'os'
import { tempDir } from '@pnpm/prepare'
import { setup } from '@pnpm/plugin-commands-setup'

jest.mock('os', () => {
  const os = jest.requireActual('os')
  return {
    ...os,
    homedir: jest.fn(),
  }
})

test('PNPM_HOME is added to ~/.bashrc', async () => {
  tempDir()
  fs.writeFileSync('.bashrc', '', 'utf8')
  homedir['mockReturnValue'](process.cwd())
  await setup.handler()
  const bashRCContent = fs.readFileSync('.bashrc', 'utf8')
  expect(bashRCContent).toEqual(`
export PNPM_HOME="${__dirname}"
export PATH="$PNPM_HOME:$PATH"
`)
})

test('PNPM_HOME is not added to ~/.bashrc if already present', async () => {
  tempDir()
  fs.writeFileSync('.bashrc', `
export PNPM_HOME="pnpm_home"
export PATH="$PNPM_HOME:$PATH"
`, 'utf8')
  homedir['mockReturnValue'](process.cwd())
  await setup.handler()
  const bashRCContent = fs.readFileSync('.bashrc', 'utf8')
  expect(bashRCContent).toEqual(`
export PNPM_HOME="pnpm_home"
export PATH="$PNPM_HOME:$PATH"
`)
})
