// cspell:ignore ZDOTDIR esep
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import util from 'node:util'

import { PnpmError } from '@pnpm/error'
import writeFileAtomic from 'write-file-atomic'

export class BadShellSectionError extends PnpmError {
  public current: string
  public wanted: string
  constructor (opts: { configSectionName: string, wanted: string, current: string, configFile: string }) {
    super('BAD_SHELL_SECTION', `The config file at "${opts.configFile}" already contains a ${opts.configSectionName} section but with other configuration`)
    this.current = opts.current
    this.wanted = opts.wanted
  }
}

export type AddingPosition = 'start' | 'end'

export interface AddDirToPosixEnvPathOpts {
  proxyVarName?: string
  proxyVarSubDir?: string
  overwrite?: boolean
  position?: AddingPosition
  configSectionName: string
}

export type ConfigFileChangeType = 'skipped' | 'appended' | 'modified' | 'created'

export interface ConfigReport {
  path: string
  changeType: ConfigFileChangeType
}

export interface PathExtenderPosixReport {
  configFile: ConfigReport
  oldSettings: string
  newSettings: string
}

export async function addDirToPosixEnvPath (
  dir: string,
  opts: AddDirToPosixEnvPathOpts
): Promise<PathExtenderPosixReport> {
  const currentShell = detectCurrentShell()
  return updateShell(currentShell, dir, opts)
}

function detectCurrentShell (): string | null {
  if (process.env.ZSH_VERSION) return 'zsh'
  if (process.env.BASH_VERSION) return 'bash'
  if (process.env.FISH_VERSION) return 'fish'
  if (process.env.NU_VERSION) return 'nu'
  return typeof process.env.SHELL === 'string' ? path.basename(process.env.SHELL) : null
}

async function updateShell (
  currentShell: string | null,
  pnpmHomeDir: string,
  opts: AddDirToPosixEnvPathOpts
): Promise<PathExtenderPosixReport> {
  switch (currentShell) {
    case 'bash':
    case 'zsh':
    case 'ksh':
    case 'dash':
    case 'sh': {
      return setupShell(currentShell, pnpmHomeDir, opts)
    }
    case 'fish': {
      return setupFishShell(pnpmHomeDir, opts)
    }
    case 'nu': {
      return setupNuShell(pnpmHomeDir, opts)
    }
  }
  const supportedShellsMsg = 'Supported shell languages are bash, zsh, fish, ksh, dash, sh, and nushell.'
  if (!currentShell) {
    throw new PnpmError('UNKNOWN_SHELL', 'Could not infer shell type.', {
      hint: `Set the SHELL environment variable to your active shell.\n${supportedShellsMsg}`,
    })
  }
  throw new PnpmError('UNSUPPORTED_SHELL', `Can't setup configuration for "${currentShell}" shell`, {
    hint: supportedShellsMsg,
  })
}

async function setupShell (
  shell: 'bash' | 'zsh' | 'ksh' | 'dash' | 'sh',
  dir: string,
  opts: AddDirToPosixEnvPathOpts
): Promise<PathExtenderPosixReport> {
  const configFile = getConfigFilePath(shell)
  let newSettings: string
  const _createPathValue = createPathValue.bind(null, opts.position ?? 'start')
  // Always export PATH. Skipping the export when the directory is already
  // present leaves a later entry ahead of pnpm after macOS VS Code sources
  // ~/.zshrc again.
  if (opts.proxyVarName) {
    const pathRef = opts.proxyVarSubDir ? `$${opts.proxyVarName}/${opts.proxyVarSubDir}` : `$${opts.proxyVarName}`
    newSettings = `export ${opts.proxyVarName}="${dir}"
export PATH="${_createPathValue(pathRef)}"`
  } else {
    newSettings = `export PATH="${_createPathValue(dir)}"`
  }
  const content = wrapSettings(opts.configSectionName, newSettings)
  const { changeType, oldSettings } = await updateShellConfig(configFile, content, opts)
  return {
    configFile: {
      path: configFile,
      changeType,
    },
    oldSettings,
    newSettings,
  }
}

function getConfigFilePath (shell: 'bash' | 'zsh' | 'ksh' | 'dash' | 'sh'): string {
  switch (shell) {
    case 'zsh': return path.join((process.env.ZDOTDIR || os.homedir()), `.${shell}rc`)
    case 'dash':
    case 'sh': {
      if (!process.env.ENV) {
        throw new PnpmError('NO_SHELL_CONFIG', `Cannot find a config file for ${shell}. The ENV environment variable is not set.`)
      }
      return process.env.ENV
    }
    default: return path.join(os.homedir(), `.${shell}rc`)
  }
}

function createPathValue (position: AddingPosition, dir: string): string {
  return position === 'start'
    ? `${dir}:$PATH`
    : `$PATH:${dir}`
}

async function setupFishShell (dir: string, opts: AddDirToPosixEnvPathOpts): Promise<PathExtenderPosixReport> {
  const configFile = path.join(os.homedir(), '.config/fish/config.fish')
  let newSettings: string
  const _createPathValue = createFishPathValue.bind(null, opts.position ?? 'start')
  if (opts.proxyVarName) {
    const pathRef = opts.proxyVarSubDir ? `$${opts.proxyVarName}/${opts.proxyVarSubDir}` : `$${opts.proxyVarName}`
    newSettings = `set -gx ${opts.proxyVarName} "${dir}"
set -gx PATH ${_createPathValue(pathRef)}`
  } else {
    newSettings = `set -gx PATH ${_createPathValue(dir)}`
  }
  const content = wrapSettings(opts.configSectionName, newSettings)
  const { changeType, oldSettings } = await updateShellConfig(configFile, content, opts)
  return {
    configFile: {
      path: configFile,
      changeType,
    },
    oldSettings,
    newSettings,
  }
}

async function setupNuShell (dir: string, opts: AddDirToPosixEnvPathOpts): Promise<PathExtenderPosixReport> {
  const configFile = path.join(os.homedir(), '.config/nushell/env.nu')
  let newSettings: string
  const addingCommand = (opts.position ?? 'start') === 'start' ? 'prepend' : 'append'
  if (opts.proxyVarName) {
    const pathRef = opts.proxyVarSubDir
      ? `($env.${opts.proxyVarName} | path join "${opts.proxyVarSubDir}")`
      : `$env.${opts.proxyVarName}`
    newSettings = `$env.${opts.proxyVarName} = "${dir}"
$env.PATH = ($env.PATH | split row (char esep) | ${addingCommand} ${pathRef} )`
  } else {
    newSettings = `$env.PATH = ($env.PATH | split row (char esep) | ${addingCommand} ${dir} )`
  }
  const content = wrapSettings(opts.configSectionName, newSettings)
  const { changeType, oldSettings } = await updateShellConfig(configFile, content, opts)
  return {
    configFile: {
      path: configFile,
      changeType,
    },
    oldSettings,
    newSettings,
  }
}

export function wrapSettings (sectionName: string, settings: string): string {
  return `# ${sectionName}
${settings}
# ${sectionName} end`
}

function createFishPathValue (position: AddingPosition, dir: string): string {
  return position === 'start'
    ? `"${dir}" $PATH`
    : `$PATH "${dir}"`
}

interface UpdateShellResult {
  changeType: ConfigFileChangeType
  oldSettings: string
}

export interface FoundSection {
  start: number
  end: number
  inner: string
  fullMatch: string
}

/**
 * Locate the `# <section>` ... `# <section> end` block.
 *
 * A valid section is bounded by an opening `# <section>` line and a closing
 * `# <section> end` line with no intermediate `# <section>` or `# <section> end`
 * markers. If several valid sections exist, the last one whose non-comment
 * lines reference both `PATH` and `<SECTION>_HOME` wins, then the last one
 * referencing `<SECTION>_HOME`, then the last one referencing `PATH`, then the
 * last section.
 */
export function findSection (content: string, section: string): FoundSection | null {
  if (!content) return null
  const startMarker = `# ${section}`
  const endMarker = `# ${section} end`

  const sections: FoundSection[] = []
  let lastStart: { lineStart: number, innerStart: number } | null = null
  let offset = 0

  const lines = content.split('\n')
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i]
    const lineStart = offset
    const lineLengthWithNewline = i < lines.length - 1 ? line.length + 1 : line.length
    offset += lineLengthWithNewline

    const trimmed = line.replace(/[\r \t]+$/, '')
    if (trimmed === startMarker) {
      lastStart = {
        lineStart,
        innerStart: offset,
      }
    } else if (trimmed === endMarker) {
      if (lastStart) {
        const { lineStart: startOffset, innerStart } = lastStart
        lastStart = null

        const inner = content.slice(innerStart, lineStart).replace(/[\r\n]+$/, '')
        const markerLen = line.replace(/[\r\n]+$/, '').length
        const rangeEnd = lineStart + markerLen
        sections.push({
          start: startOffset,
          end: rangeEnd,
          inner,
          fullMatch: content.slice(startOffset, rangeEnd),
        })
      }
    }
  }

  if (sections.length === 0) return null
  if (sections.length === 1) return sections[0]

  const homeVar = `${section.toUpperCase()}_HOME`
  const settings = sections.map(({ inner }) => stripComments(inner))
  const predicates: Array<(text: string) => boolean> = [
    (text) => text.includes('PATH') && text.includes(homeVar),
    (text) => text.includes(homeVar),
    (text) => text.includes('PATH'),
  ]
  for (const predicate of predicates) {
    for (let i = sections.length - 1; i >= 0; i--) {
      if (predicate(settings[i])) return sections[i]
    }
  }
  return sections[sections.length - 1]
}

function stripComments (settings: string): string {
  return settings.split('\n').filter((line) => !line.trimStart().startsWith('#')).join('\n')
}

export function replaceSection (originalContent: string, newSection: string, sectionName: string): string {
  const section = findSection(originalContent, sectionName)
  if (!section) return originalContent
  return originalContent.slice(0, section.start) + newSection + originalContent.slice(section.end)
}

export async function updateShellConfig (
  configFile: string,
  newContent: string,
  opts: AddDirToPosixEnvPathOpts
): Promise<UpdateShellResult> {
  await fs.promises.mkdir(path.dirname(configFile), { recursive: true })
  try {
    await fs.promises.writeFile(configFile, `${newContent}\n`, { encoding: 'utf8', flag: 'wx' })
    return {
      changeType: 'created',
      oldSettings: '',
    }
  } catch (err: unknown) {
    if (!util.types.isNativeError(err) || !('code' in err) || err.code !== 'EEXIST') {
      throw err
    }
  }
  const configContent = await fs.promises.readFile(configFile, 'utf8')
  const section = findSection(configContent, opts.configSectionName)
  if (!section) {
    await fs.promises.appendFile(configFile, `\n${newContent}\n`, 'utf8')
    return {
      changeType: 'appended',
      oldSettings: '',
    }
  }
  const oldSettings = section.inner
  const normalizedFullMatch = section.fullMatch.replace(/\r\n/g, '\n')
  if (normalizedFullMatch !== newContent) {
    if (!opts.overwrite) {
      throw new BadShellSectionError({
        configSectionName: opts.configSectionName,
        current: section.fullMatch,
        wanted: newContent,
        configFile,
      })
    }
    const newConfigContent = configContent.slice(0, section.start) + newContent + configContent.slice(section.end)
    await writeFileAtomic(configFile, newConfigContent, 'utf8')
    return {
      changeType: 'modified',
      oldSettings,
    }
  }
  return {
    changeType: 'skipped',
    oldSettings,
  }
}
