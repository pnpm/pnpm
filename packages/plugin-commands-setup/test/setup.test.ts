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
  process.env.SHELL = '/bin/bash'
  tempDir()
  fs.writeFileSync('.bashrc', '', 'utf8')
  homedir['mockReturnValue'](process.cwd())
  const output = await setup.handler({
    pnpmHomeDir: __dirname,
  })
  expect(output).toMatch(/^Updated /)
  const bashRCContent = fs.readFileSync('.bashrc', 'utf8')
  expect(bashRCContent).toEqual(`
export PNPM_HOME="${__dirname}"
export PATH="$PNPM_HOME:$PATH"
`)
})

test('PNPM_HOME is not added to ~/.bashrc if already present', async () => {
  process.env.SHELL = '/bin/bash'
  tempDir()
  fs.writeFileSync('.bashrc', `
export PNPM_HOME="pnpm_home"
export PATH="$PNPM_HOME:$PATH"
`, 'utf8')
  homedir['mockReturnValue'](process.cwd())
  const output = await setup.handler({
    pnpmHomeDir: __dirname,
  })
  expect(output).toMatch(/^PNPM_HOME is already in /)
  const bashRCContent = fs.readFileSync('.bashrc', 'utf8')
  expect(bashRCContent).toEqual(`
export PNPM_HOME="pnpm_home"
export PATH="$PNPM_HOME:$PATH"
`)
})

test('PNPM_HOME is added to ~/.zshrc', async () => {
  process.env.SHELL = '/bin/zsh'
  tempDir()
  fs.writeFileSync('.zshrc', '', 'utf8')
  homedir['mockReturnValue'](process.cwd())
  const output = await setup.handler({
    pnpmHomeDir: __dirname,
  })
  expect(output).toMatch(/^Updated /)
  const bashRCContent = fs.readFileSync('.zshrc', 'utf8')
  expect(bashRCContent).toEqual(`
export PNPM_HOME="${__dirname}"
export PATH="$PNPM_HOME:$PATH"
`)
})

test('PNPM_HOME is not added to ~/.zshrc if already present', async () => {
  process.env.SHELL = '/bin/zsh'
  tempDir()
  fs.writeFileSync('.zshrc', `
export PNPM_HOME="pnpm_home"
export PATH="$PNPM_HOME:$PATH"
`, 'utf8')
  homedir['mockReturnValue'](process.cwd())
  const output = await setup.handler({
    pnpmHomeDir: __dirname,
  })
  expect(output).toMatch(/^PNPM_HOME is already in /)
  const bashRCContent = fs.readFileSync('.zshrc', 'utf8')
  expect(bashRCContent).toEqual(`
export PNPM_HOME="pnpm_home"
export PATH="$PNPM_HOME:$PATH"
`)
})

test('PNPM_HOME is added to ~/.config/fish/config.fish', async () => {
  process.env.SHELL = '/bin/fish'
  tempDir()
  fs.mkdirSync('.config/fish', { recursive: true })
  fs.writeFileSync('.config/fish/config.fish', '', 'utf8')
  homedir['mockReturnValue'](process.cwd())
  const output = await setup.handler({
    pnpmHomeDir: __dirname,
  })
  expect(output).toMatch(/^Updated /)
  const bashRCContent = fs.readFileSync('.config/fish/config.fish', 'utf8')
  expect(bashRCContent).toEqual(`
set -gx PNPM_HOME "${__dirname}"
set -gx PATH "$PNPM_HOME" $PATH
`)
})

test('PNPM_HOME is not added to ~/.config/fish/config.fish if already present', async () => {
  process.env.SHELL = '/bin/fish'
  tempDir()
  fs.mkdirSync('.config/fish', { recursive: true })
  fs.writeFileSync('.config/fish/config.fish', `
set -gx PNPM_HOME "pnpm_home"
set -gx PATH "$PNPM_HOME" $PATH
`, 'utf8')
  homedir['mockReturnValue'](process.cwd())
  const output = await setup.handler({
    pnpmHomeDir: __dirname,
  })
  expect(output).toMatch(/^PNPM_HOME is already in /)
  const bashRCContent = fs.readFileSync('.config/fish/config.fish', 'utf8')
  expect(bashRCContent).toEqual(`
set -gx PNPM_HOME "pnpm_home"
set -gx PATH "$PNPM_HOME" $PATH
`)
})
