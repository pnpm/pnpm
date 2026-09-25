import { spawnSync } from 'node:child_process'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { describe, expect, test } from '@jest/globals'
import { PnpmError } from '@pnpm/error'

import {
  addDirToPosixEnvPath,
  type AddDirToPosixEnvPathOpts,
  findSection,
  replaceSection,
  updateShellConfig,
  wrapSettings,
} from '../../src/setup/pathExtenderPosix.js'

function opts (overwrite: boolean): AddDirToPosixEnvPathOpts {
  return {
    configSectionName: 'pnpm',
    proxyVarName: 'PNPM_HOME',
    overwrite,
    position: 'start',
  }
}

describe('pathExtenderPosix', () => {
  test('findSection extracts inner settings and range', () => {
    const content = 'head\n# pnpm\nbody line 1\nbody line 2\n# pnpm end\ntail'
    const section = findSection(content, 'pnpm')
    expect(section).not.toBeNull()
    expect(section!.inner).toBe('body line 1\nbody line 2')
    expect(content.slice(section!.start, section!.end)).toBe('# pnpm\nbody line 1\nbody line 2\n# pnpm end')
  })

  test('findSection returns null when absent', () => {
    expect(findSection('no markers here', 'pnpm')).toBeNull()
  })

  test('replaceSection swaps the block', () => {
    const content = 'a\n# pnpm\nold\n# pnpm end\nb'
    const replaced = replaceSection(content, '# pnpm\nnew\n# pnpm end', 'pnpm')
    expect(replaced).toBe('a\n# pnpm\nnew\n# pnpm end\nb')
  })

  test('updateShellConfig creates config file when it does not exist', async () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-posix-test-'))
    try {
      const configFile = path.join(dir, 'sub', '.bashrc')
      const content = wrapSettings('pnpm', 'export FOO=1')

      const result = await updateShellConfig(configFile, content, opts(false))
      expect(result.changeType).toBe('created')
      expect(result.oldSettings).toBe('')

      const written = fs.readFileSync(configFile, 'utf8')
      expect(written).toBe('# pnpm\nexport FOO=1\n# pnpm end\n')
    } finally {
      fs.rmSync(dir, { recursive: true, force: true })
    }
  })

  test('updateShellConfig appends to an existing config file', async () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-posix-test-'))
    try {
      const configFile = path.join(dir, '.bashrc')
      fs.writeFileSync(configFile, '')
      const content = wrapSettings('pnpm', 'export FOO=1')

      const result = await updateShellConfig(configFile, content, opts(false))
      expect(result.changeType).toBe('appended')
      expect(result.oldSettings).toBe('')

      const written = fs.readFileSync(configFile, 'utf8')
      expect(written).toBe('\n# pnpm\nexport FOO=1\n# pnpm end\n')
    } finally {
      fs.rmSync(dir, { recursive: true, force: true })
    }
  })

  test('updateShellConfig skips when config is already present', async () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-posix-test-'))
    try {
      const configFile = path.join(dir, '.bashrc')
      const content = wrapSettings('pnpm', 'export FOO=1')
      fs.writeFileSync(configFile, `\n${content}\n`)

      const result = await updateShellConfig(configFile, content, opts(false))
      expect(result.changeType).toBe('skipped')
      expect(result.oldSettings).toBe('export FOO=1')
    } finally {
      fs.rmSync(dir, { recursive: true, force: true })
    }
  })

  test('updateShellConfig fails when section differs and overwrite is off', async () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-posix-test-'))
    try {
      const configFile = path.join(dir, '.bashrc')
      const existing = wrapSettings('pnpm', 'export FOO=old')
      fs.writeFileSync(configFile, `\n${existing}\n`)
      const content = wrapSettings('pnpm', 'export FOO=new')

      await expect(updateShellConfig(configFile, content, opts(false)))
        .rejects.toThrow(PnpmError)

      const written = fs.readFileSync(configFile, 'utf8')
      expect(written).toBe(`\n${existing}\n`)
    } finally {
      fs.rmSync(dir, { recursive: true, force: true })
    }
  })

  test('updateShellConfig replaces when section differs and overwrite is on', async () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-posix-test-'))
    try {
      const configFile = path.join(dir, '.bashrc')
      const existing = wrapSettings('pnpm', 'export FOO=old')
      fs.writeFileSync(configFile, `before\n${existing}\nafter\n`)
      const content = wrapSettings('pnpm', 'export FOO=new')

      const result = await updateShellConfig(configFile, content, opts(true))
      expect(result.changeType).toBe('modified')
      expect(result.oldSettings).toBe('export FOO=old')

      const written = fs.readFileSync(configFile, 'utf8')
      expect(written).toBe(`before\n${content}\nafter\n`)
    } finally {
      fs.rmSync(dir, { recursive: true, force: true })
    }
  })

  test('preserves surrounding aliases and nvm lines when updating (pnpm/pnpm#7067)', async () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-posix-test-'))
    try {
      const configFile = path.join(dir, '.zshrc')
      const preamble = `# pnpm
# https://pnpm.io/installation#using-a-shorter-alias
alias p="pnpm"
alias px="pnpm dlx"
alias s='pnpm create svelte@latest'
alias pi='pnpm install'
alias pa='pnpm add'

export NVM_DIR="$([ -z "\${XDG_CONFIG_HOME-}" ] && printf %s "\${HOME}/.nvm" || printf %s "\${XDG_CONFIG_HOME}/nvm")"
[ -s "$NVM_DIR/nvm.sh" ] && \\. "$NVM_DIR/nvm.sh"

`
      const oldSection = wrapSettings(
        'pnpm',
        `export PNPM_HOME="/home/user/.old_pnpm"
case ":$PATH:" in
  *":$PNPM_HOME:"*) ;;
  *) export PATH="$PNPM_HOME:$PATH" ;;
esac`
      )
      fs.writeFileSync(configFile, `${preamble}${oldSection}\n`)

      const newSection = wrapSettings(
        'pnpm',
        `export PNPM_HOME="/home/user/.new_pnpm"
case ":$PATH:" in
  *":$PNPM_HOME:"*) ;;
  *) export PATH="$PNPM_HOME:$PATH" ;;
esac`
      )

      const result = await updateShellConfig(configFile, newSection, opts(true))
      expect(result.changeType).toBe('modified')
      expect(result.oldSettings).toBe(`export PNPM_HOME="/home/user/.old_pnpm"
case ":$PATH:" in
  *":$PNPM_HOME:"*) ;;
  *) export PATH="$PNPM_HOME:$PATH" ;;
esac`)

      const written = fs.readFileSync(configFile, 'utf8')
      expect(written).toBe(`${preamble}${newSection}\n`)
    } finally {
      fs.rmSync(dir, { recursive: true, force: true })
    }
  })

  test('appends when only a comment marker is present', async () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-posix-test-'))
    try {
      const configFile = path.join(dir, '.zshrc')
      const initial = '# pnpm\nalias p="pnpm"\n'
      fs.writeFileSync(configFile, initial)

      const newSection = wrapSettings('pnpm', 'export FOO=1')
      const result = await updateShellConfig(configFile, newSection, opts(false))

      expect(result.changeType).toBe('appended')
      expect(result.oldSettings).toBe('')

      const written = fs.readFileSync(configFile, 'utf8')
      expect(written).toBe(`${initial}\n${newSection}\n`)
    } finally {
      fs.rmSync(dir, { recursive: true, force: true })
    }
  })

  test('selects path section when multiple sections exist', async () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-posix-test-'))
    try {
      const configFile = path.join(dir, '.zshrc')
      const aliasBlock = wrapSettings('pnpm', 'alias p=pnpm')
      const oldEnvBlock = wrapSettings('pnpm', 'export PNPM_HOME=/old\nexport PATH=$PNPM_HOME:$PATH')
      const initial = `${aliasBlock}\n\nexport OTHER=1\n\n${oldEnvBlock}\n`
      fs.writeFileSync(configFile, initial)

      const newEnvBlock = wrapSettings('pnpm', 'export PNPM_HOME=/new\nexport PATH=$PNPM_HOME:$PATH')
      const result = await updateShellConfig(configFile, newEnvBlock, opts(true))

      expect(result.changeType).toBe('modified')
      expect(result.oldSettings).toBe('export PNPM_HOME=/old\nexport PATH=$PNPM_HOME:$PATH')

      const written = fs.readFileSync(configFile, 'utf8')
      expect(written).toBe(`${aliasBlock}\n\nexport OTHER=1\n\n${newEnvBlock}\n`)
    } finally {
      fs.rmSync(dir, { recursive: true, force: true })
    }
  })

  test('prioritizes path block over later block mentioning pnpm', async () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-posix-test-'))
    try {
      const configFile = path.join(dir, '.zshrc')
      const oldEnvBlock = wrapSettings('pnpm', 'export PNPM_HOME=/old\nexport PATH=$PNPM_HOME:$PATH')
      const aliasBlock = wrapSettings('pnpm', '# PNPM aliases\nalias p=pnpm')
      const initial = `${oldEnvBlock}\n\nexport OTHER=1\n\n${aliasBlock}\n`
      fs.writeFileSync(configFile, initial)

      const newEnvBlock = wrapSettings('pnpm', 'export PNPM_HOME=/new\nexport PATH=$PNPM_HOME:$PATH')
      const result = await updateShellConfig(configFile, newEnvBlock, opts(true))

      expect(result.changeType).toBe('modified')
      expect(result.oldSettings).toBe('export PNPM_HOME=/old\nexport PATH=$PNPM_HOME:$PATH')

      const written = fs.readFileSync(configFile, 'utf8')
      expect(written).toBe(`${newEnvBlock}\n\nexport OTHER=1\n\n${aliasBlock}\n`)
    } finally {
      fs.rmSync(dir, { recursive: true, force: true })
    }
  })

  test('prioritizes generated env block over later custom path block', async () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-posix-test-'))
    try {
      const configFile = path.join(dir, '.zshrc')
      const generatedEnvBlock = wrapSettings('pnpm', 'export PNPM_HOME=/old\nexport PATH=$PNPM_HOME:$PATH')
      const customPathBlock = wrapSettings('pnpm', 'export PATH=/custom/bin:$PATH')
      const initial = `${generatedEnvBlock}\n\nexport OTHER=1\n\n${customPathBlock}\n`
      fs.writeFileSync(configFile, initial)

      const newEnvBlock = wrapSettings('pnpm', 'export PNPM_HOME=/new\nexport PATH=$PNPM_HOME:$PATH')
      const result = await updateShellConfig(configFile, newEnvBlock, opts(true))

      expect(result.changeType).toBe('modified')
      expect(result.oldSettings).toBe('export PNPM_HOME=/old\nexport PATH=$PNPM_HOME:$PATH')

      const written = fs.readFileSync(configFile, 'utf8')
      expect(written).toBe(`${newEnvBlock}\n\nexport OTHER=1\n\n${customPathBlock}\n`)
    } finally {
      fs.rmSync(dir, { recursive: true, force: true })
    }
  })

  test('ignores comments that mention PNPM_HOME and PATH when selecting the env block', async () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-posix-test-'))
    try {
      const configFile = path.join(dir, '.zshrc')
      const generatedEnvBlock = wrapSettings('pnpm', 'export PNPM_HOME=/old\nexport PATH=$PNPM_HOME:$PATH')
      const customBlock = wrapSettings('pnpm', '# Keep PNPM_HOME and PATH customizations\nalias pn=pnpm')
      fs.writeFileSync(configFile, `${generatedEnvBlock}\n\n${customBlock}\n`)

      const newEnvBlock = wrapSettings('pnpm', 'export PNPM_HOME=/new\nexport PATH=$PNPM_HOME:$PATH')
      const result = await updateShellConfig(configFile, newEnvBlock, opts(true))

      expect(result.oldSettings).toBe('export PNPM_HOME=/old\nexport PATH=$PNPM_HOME:$PATH')
      expect(fs.readFileSync(configFile, 'utf8')).toBe(`${newEnvBlock}\n\n${customBlock}\n`)
    } finally {
      fs.rmSync(dir, { recursive: true, force: true })
    }
  })

  test('updates a symlinked config file through the link', async () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-posix-test-'))
    try {
      const target = path.join(dir, 'dotfiles', 'zshrc')
      fs.mkdirSync(path.dirname(target))
      fs.writeFileSync(target, `alias ll="ls -l"\n${wrapSettings('pnpm', 'export PNPM_HOME=/old')}\n`)
      const configFile = path.join(dir, '.zshrc')
      fs.symlinkSync(target, configFile)

      const newBlock = wrapSettings('pnpm', 'export PNPM_HOME=/new')
      await updateShellConfig(configFile, newBlock, opts(true))

      expect(fs.lstatSync(configFile).isSymbolicLink()).toBe(true)
      expect(fs.readFileSync(target, 'utf8')).toBe(`alias ll="ls -l"\n${newBlock}\n`)
    } finally {
      fs.rmSync(dir, { recursive: true, force: true })
    }
  })

  test('skips equivalent CRLF section when overwrite is disabled', async () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-posix-test-'))
    try {
      const configFile = path.join(dir, '.zshrc')
      const settings = 'export PNPM_HOME=/home/user/.pnpm\nexport PATH=$PNPM_HOME:$PATH'
      const lfBlock = wrapSettings('pnpm', settings)
      const crlfBlock = lfBlock.replace(/\n/g, '\r\n')
      fs.writeFileSync(configFile, crlfBlock)

      const result = await updateShellConfig(configFile, lfBlock, opts(false))

      expect(result.changeType).toBe('skipped')
    } finally {
      fs.rmSync(dir, { recursive: true, force: true })
    }
  })

  // cspell:ignore ZDOTDIR
  test('zsh snippet prepends PNPM_HOME when it is already on PATH', async () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-posix-test-'))
    const savedConfigDir = process.env.ZDOTDIR
    const previousZshVersion = process.env.ZSH_VERSION
    process.env.ZDOTDIR = dir
    process.env.ZSH_VERSION = '5.9'
    const pnpmHome = '/home/user/.local/share/pnpm'
    try {
      const report = await addDirToPosixEnvPath(pnpmHome, {
        configSectionName: 'pnpm',
        proxyVarName: 'PNPM_HOME',
        proxyVarSubDir: 'bin',
        position: 'start',
      })
      expect(report.newSettings).toBe(`export PNPM_HOME="${pnpmHome}"
export PATH="$PNPM_HOME/bin:$PATH"`)
      const script = `PATH="/usr/local/bin:${pnpmHome}/bin:/usr/bin"
${report.newSettings}
printf '%s' "$PATH"
`
      const result = spawnSync('sh', ['-c', script], { encoding: 'utf8' })
      expect(result.status).toBe(0)
      expect(result.stdout).toBe(`${pnpmHome}/bin:/usr/local/bin:${pnpmHome}/bin:/usr/bin`)
    } finally {
      if (savedConfigDir == null) {
        delete process.env.ZDOTDIR
      } else {
        process.env.ZDOTDIR = savedConfigDir
      }
      if (previousZshVersion == null) {
        delete process.env.ZSH_VERSION
      } else {
        process.env.ZSH_VERSION = previousZshVersion
      }
      fs.rmSync(dir, { recursive: true, force: true })
    }
  })
})
