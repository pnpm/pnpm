import fs from 'fs'
import path from 'path'

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

describe('Bash', () => {
  it('should append to empty shell script', async () => {
    process.env.SHELL = '/bin/bash'
    const homeDir = tempDir()
    const pnpmHomeDir = path.join(homeDir, '.pnpm')
    fs.writeFileSync('.bashrc', '', 'utf8')
    homedir['mockReturnValue'](homeDir)
    const output = await setup.handler({ pnpmHomeDir })
    expect(output).toMatch(/^Updated /)
    const bashRCContent = fs.readFileSync('.bashrc', 'utf8')
    expect(bashRCContent).toEqual(`
# pnpm
export PNPM_HOME="${pnpmHomeDir}"
export PATH="$PNPM_HOME:$PATH"
# pnpm end
`)
  })
  it('should create a shell script', async () => {
    process.env.SHELL = '/bin/bash'
    const homeDir = tempDir()
    const pnpmHomeDir = path.join(homeDir, '.pnpm')
    homedir['mockReturnValue'](homeDir)
    const output = await setup.handler({ pnpmHomeDir })
    expect(output).toMatch(/^Created /)
    const bashRCContent = fs.readFileSync('.bashrc', 'utf8')
    expect(bashRCContent).toEqual(`# pnpm
export PNPM_HOME="${pnpmHomeDir}"
export PATH="$PNPM_HOME:$PATH"
# pnpm end
`)
  })
  it('should make no changes to a shell script that already has the necessary configurations', async () => {
    process.env.SHELL = '/bin/bash'
    const homeDir = tempDir()
    const pnpmHomeDir = path.join(homeDir, '.pnpm')
    fs.writeFileSync('.bashrc', `
export PNPM_HOME="${pnpmHomeDir}"
export PATH="$PNPM_HOME:$PATH"
`, 'utf8')
    homedir['mockReturnValue'](homeDir)
    const output = await setup.handler({ pnpmHomeDir })
    expect(output).toMatch(/^PNPM_HOME is already in /)
    const bashRCContent = fs.readFileSync('.bashrc', 'utf8')
    expect(bashRCContent).toEqual(`
export PNPM_HOME="${pnpmHomeDir}"
export PATH="$PNPM_HOME:$PATH"
`)
  })
  it('should fail if the shell already has PNPM_HOME set to a different directory', async () => {
    process.env.SHELL = '/bin/bash'
    const homeDir = tempDir()
    const pnpmHomeDir = path.join(homeDir, '.pnpm')
    fs.writeFileSync('.bashrc', `
export PNPM_HOME="pnpm_home"
export PATH="$PNPM_HOME:$PATH"
`, 'utf8')
    homedir['mockReturnValue'](homeDir)
    await expect(
      setup.handler({ pnpmHomeDir })
    ).rejects.toThrowError(/Currently 'PNPM_HOME' is set to/)
  })
  it('should not fail if setup is forced', async () => {
    process.env.SHELL = '/bin/bash'
    const homeDir = tempDir()
    const pnpmHomeDir = path.join(homeDir, '.pnpm')
    fs.writeFileSync('.bashrc', `
# pnpm
export PNPM_HOME="pnpm_home"
export PATH="$PNPM_HOME:$PATH"
# pnpm end`, 'utf8')
    homedir['mockReturnValue'](homeDir)
    const output = await setup.handler({ force: true, pnpmHomeDir })
    expect(output).toMatch(/^Updated /)
    const bashRCContent = fs.readFileSync('.bashrc', 'utf8')
    expect(bashRCContent).toEqual(`
# pnpm
export PNPM_HOME="${pnpmHomeDir}"
export PATH="$PNPM_HOME:$PATH"
# pnpm end
`)
  })
})

describe('Zsh', () => {
  it('should append to empty shell script', async () => {
    process.env.SHELL = '/bin/zsh'
    const homeDir = tempDir()
    const pnpmHomeDir = path.join(homeDir, '.pnpm')
    fs.writeFileSync('.zshrc', '', 'utf8')
    homedir['mockReturnValue'](homeDir)
    const output = await setup.handler({ pnpmHomeDir })
    expect(output).toMatch(/^Updated /)
    const bashRCContent = fs.readFileSync('.zshrc', 'utf8')
    expect(bashRCContent).toEqual(`
# pnpm
export PNPM_HOME="${pnpmHomeDir}"
export PATH="$PNPM_HOME:$PATH"
# pnpm end
`)
  })
  it('should make no changes to a shell script that already has the necessary configurations', async () => {
    process.env.SHELL = '/bin/zsh'
    const homeDir = tempDir()
    const pnpmHomeDir = path.join(homeDir, '.pnpm')
    fs.writeFileSync('.zshrc', `
export PNPM_HOME="${pnpmHomeDir}"
export PATH="$PNPM_HOME:$PATH"
`, 'utf8')
    homedir['mockReturnValue'](homeDir)
    const output = await setup.handler({ pnpmHomeDir })
    expect(output).toMatch(/^PNPM_HOME is already in /)
    const bashRCContent = fs.readFileSync('.zshrc', 'utf8')
    expect(bashRCContent).toEqual(`
export PNPM_HOME="${pnpmHomeDir}"
export PATH="$PNPM_HOME:$PATH"
`)
  })
})

describe('Fish', () => {
  it('should append to empty shell script', async () => {
    process.env.SHELL = '/bin/fish'
    const homeDir = tempDir()
    const pnpmHomeDir = path.join(homeDir, '.pnpm')
    fs.mkdirSync('.config/fish', { recursive: true })
    fs.writeFileSync('.config/fish/config.fish', '', 'utf8')
    homedir['mockReturnValue'](homeDir)
    const output = await setup.handler({ pnpmHomeDir })
    expect(output).toMatch(/^Updated /)
    const bashRCContent = fs.readFileSync('.config/fish/config.fish', 'utf8')
    expect(bashRCContent).toEqual(`
# pnpm
set -gx PNPM_HOME "${pnpmHomeDir}"
set -gx PATH "$PNPM_HOME" $PATH
# pnpm end
`)
  })
  it('should create a shell script', async () => {
    process.env.SHELL = '/bin/fish'
    const homeDir = tempDir()
    const pnpmHomeDir = path.join(homeDir, '.pnpm')
    fs.mkdirSync('.config/fish', { recursive: true })
    homedir['mockReturnValue'](homeDir)
    const output = await setup.handler({ pnpmHomeDir })
    expect(output).toMatch(/^Created /)
    const bashRCContent = fs.readFileSync('.config/fish/config.fish', 'utf8')
    expect(bashRCContent).toEqual(`# pnpm
set -gx PNPM_HOME "${pnpmHomeDir}"
set -gx PATH "$PNPM_HOME" $PATH
# pnpm end
`)
  })
  it('should make no changes to a shell script that already has the necessary configurations', async () => {
    process.env.SHELL = '/bin/fish'
    const homeDir = tempDir()
    const pnpmHomeDir = path.join(homeDir, '.pnpm')
    fs.mkdirSync('.config/fish', { recursive: true })
    fs.writeFileSync('.config/fish/config.fish', `
set -gx PNPM_HOME "${pnpmHomeDir}"
set -gx PATH "$PNPM_HOME" $PATH
`, 'utf8')
    homedir['mockReturnValue'](homeDir)
    const output = await setup.handler({ pnpmHomeDir })
    expect(output).toMatch(/^PNPM_HOME is already in /)
    const bashRCContent = fs.readFileSync('.config/fish/config.fish', 'utf8')
    expect(bashRCContent).toEqual(`
set -gx PNPM_HOME "${pnpmHomeDir}"
set -gx PATH "$PNPM_HOME" $PATH
`)
  })
  it('should fail if the shell already has PNPM_HOME set to a different directory', async () => {
    process.env.SHELL = '/bin/fish'
    const homeDir = tempDir()
    const pnpmHomeDir = path.join(homeDir, '.pnpm')
    fs.mkdirSync('.config/fish', { recursive: true })
    fs.writeFileSync('.config/fish/config.fish', `
set -gx PNPM_HOME "pnpm_home"
set -gx PATH "$PNPM_HOME" $PATH
`, 'utf8')
    homedir['mockReturnValue'](homeDir)
    await expect(
      setup.handler({ pnpmHomeDir })
    ).rejects.toThrowError(/Currently 'PNPM_HOME' is set to/)
  })
  it('should not fail if setup is forced', async () => {
    process.env.SHELL = '/bin/fish'
    const homeDir = tempDir()
    const pnpmHomeDir = path.join(homeDir, '.pnpm')
    fs.mkdirSync('.config/fish', { recursive: true })
    fs.writeFileSync('.config/fish/config.fish', `
# pnpm
set -gx PNPM_HOME "pnpm_home"
set -gx PATH "$PNPM_HOME" $PATH
# pnpm end`, 'utf8')
    homedir['mockReturnValue'](homeDir)
    const output = await setup.handler({ force: true, pnpmHomeDir })
    expect(output).toMatch(/^Updated /)
    const bashRCContent = fs.readFileSync('.config/fish/config.fish', 'utf8')
    expect(bashRCContent).toEqual(`
# pnpm
set -gx PNPM_HOME "${pnpmHomeDir}"
set -gx PATH "$PNPM_HOME" $PATH
# pnpm end
`)
  })
})
