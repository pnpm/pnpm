// cspell:ignore ZDOTDIR esep
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { isError, PnpmError } from '@pnpm/error'
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

/**
 * Which existing `PATH` entry makes the rendered block skip adding the dir.
 *
 * - `positioned`: the dir is already at the adding position. A login shell can
 *   reorder an inherited `PATH` before the rc file runs (macOS `path_helper`),
 *   so the dir being present elsewhere is not enough for `start`.
 * - `anywhere`: the dir is anywhere in `PATH`. Earlier pnpm versions rendered
 *   this guard, so a block rendered with it is replaced without `overwrite`.
 */
type PathGuard = 'positioned' | 'anywhere'

export interface AddDirToPosixEnvPathOpts {
  proxyVarName?: string
  proxyVarSubDir?: string
  overwrite?: boolean
  position?: AddingPosition
  configSectionName: string
}

export type ShellType = 'zsh' | 'bash' | 'fish' | 'ksh' | 'dash' | 'sh' | 'nu'

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
  const supportedShellsMsg = 'Supported shell languages are bash, zsh, fish, ksh, dash, sh, and nushell.'
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
    case null:
    case '': {
      throw new PnpmError('UNKNOWN_SHELL', 'Could not infer shell type.', {
        hint: `Set the SHELL environment variable to your active shell.\n${supportedShellsMsg}`,
      })
    }
    default: {
      throw new PnpmError('UNSUPPORTED_SHELL', `Can't setup configuration for "${currentShell}" shell`, {
        hint: supportedShellsMsg,
      })
    }
  }
}

async function setupShell (
  shell: 'bash' | 'zsh' | 'ksh' | 'dash' | 'sh',
  dir: string,
  opts: AddDirToPosixEnvPathOpts
): Promise<PathExtenderPosixReport> {
  const configFile = getConfigFilePath(shell)
  const newSettings = renderPosixSettings(dir, opts, 'positioned')
  const content = wrapSettings(opts.configSectionName, newSettings)
  const outdated = wrapSettings(opts.configSectionName, renderPosixSettings(dir, opts, 'anywhere'))
  const { changeType, oldSettings } = await updateShellConfig(configFile, content, { ...opts, outdated })
  return {
    configFile: {
      path: configFile,
      changeType,
    },
    oldSettings,
    newSettings,
  }
}

function renderPosixSettings (dir: string, opts: AddDirToPosixEnvPathOpts, guard: PathGuard): string {
  const position = opts.position ?? 'start'
  const pathRef = opts.proxyVarName
    ? (opts.proxyVarSubDir ? `$${opts.proxyVarName}/${opts.proxyVarSubDir}` : `$${opts.proxyVarName}`)
    : dir
  const guardedCase = `case ":$PATH:" in
  ${createCasePattern(position, guard, `":${pathRef}:"`)}) ;;
  *) export PATH="${createPathValue(position, pathRef)}" ;;
esac`
  return opts.proxyVarName ? `export ${opts.proxyVarName}="${dir}"\n${guardedCase}` : guardedCase
}

function createCasePattern (position: AddingPosition, guard: PathGuard, entry: string): string {
  if (guard === 'anywhere') return `*${entry}*`
  return position === 'start' ? `${entry}*` : `*${entry}`
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
  const newSettings = renderFishSettings(dir, opts, 'positioned')
  const content = wrapSettings(opts.configSectionName, newSettings)
  const outdated = wrapSettings(opts.configSectionName, renderFishSettings(dir, opts, 'anywhere'))
  const { changeType, oldSettings } = await updateShellConfig(configFile, content, { ...opts, outdated })
  return {
    configFile: {
      path: configFile,
      changeType,
    },
    oldSettings,
    newSettings,
  }
}

function renderFishSettings (dir: string, opts: AddDirToPosixEnvPathOpts, guard: PathGuard): string {
  const position = opts.position ?? 'start'
  if (!opts.proxyVarName) {
    return `if ${createFishCondition(position, guard, `"${dir}"`)}
  set -gx PATH ${createFishPathValue(position, dir)}
end`
  }
  const pathRef = opts.proxyVarSubDir ? `$${opts.proxyVarName}/${opts.proxyVarSubDir}` : `$${opts.proxyVarName}`
  // The `anywhere` block left a bare `$PROXY` unquoted.
  const entry = guard === 'anywhere' && !opts.proxyVarSubDir ? pathRef : `"${pathRef}"`
  return `set -gx ${opts.proxyVarName} "${dir}"
if ${createFishCondition(position, guard, entry)}
  set -gx PATH ${createFishPathValue(position, pathRef)}
end`
}

function createFishCondition (position: AddingPosition, guard: PathGuard, entry: string): string {
  if (guard === 'anywhere') return `not string match -q -- ${entry} $PATH`
  return `test "$PATH[${position === 'start' ? '1' : '-1'}]" != ${entry}`
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

export interface UpdateShellConfigOpts extends AddDirToPosixEnvPathOpts {
  /** The block an earlier pnpm version rendered. It is replaced without `overwrite`. */
  outdated?: string
}

export async function updateShellConfig (
  configFile: string,
  newContent: string,
  opts: UpdateShellConfigOpts
): Promise<UpdateShellResult> {
  await fs.promises.mkdir(path.dirname(configFile), { recursive: true })
  const created = await tryCreateShellConfig(configFile, newContent)
  if (created) return created

  const configContent = await fs.promises.readFile(configFile, 'utf8')
  const section = findSection(configContent, opts.configSectionName, opts.proxyVarName)
  if (!section) {
    await fs.promises.appendFile(configFile, `\n${newContent}\n`, 'utf8')
    return { changeType: 'appended', oldSettings: '' }
  }
  return applySectionUpdate(configFile, configContent, section, newContent, opts)
}

async function tryCreateShellConfig (configFile: string, newContent: string): Promise<UpdateShellResult | null> {
  try {
    await fs.promises.writeFile(configFile, `${newContent}\n`, { encoding: 'utf8', flag: 'wx' })
    return { changeType: 'created', oldSettings: '' }
  } catch (err: unknown) {
    if (!isError(err) || !('code' in err) || err.code !== 'EEXIST') {
      throw err
    }
    return null
  }
}

async function applySectionUpdate (
  configFile: string,
  configContent: string,
  section: FoundSection,
  newContent: string,
  opts: UpdateShellConfigOpts
): Promise<UpdateShellResult> {
  const oldSettings = section.inner
  const normalizedFullMatch = section.fullMatch.replace(/\r\n/g, '\n')
  if (normalizedFullMatch === newContent) {
    return { changeType: 'skipped', oldSettings }
  }
  if (!opts.overwrite && normalizedFullMatch !== opts.outdated) {
    throw new BadShellSectionError({
      configFile,
      configSectionName: opts.configSectionName,
      current: section.fullMatch,
      wanted: newContent,
    })
  }
  const newConfigContent = configContent.slice(0, section.start) + newContent + configContent.slice(section.end)
  await writeFileAtomic(configFile, newConfigContent, 'utf8')
  return { changeType: 'modified', oldSettings }
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
 * lines reference both `PATH` and `homeVar` wins, then the last one
 * referencing `homeVar`, then the last one referencing `PATH`, then the last
 * section. Returns `null` when the content holds no complete section.
 */
export function findSection (
  content: string,
  section: string,
  homeVar = `${section.toUpperCase()}_HOME`
): FoundSection | null {
  if (!content) return null
  const sections = collectCandidateSections(content, section)
  if (sections.length === 0) return null
  if (sections.length === 1) return sections[0]
  return pickBestSection(sections, homeVar)
}

function collectCandidateSections (content: string, section: string): FoundSection[] {
  const startMarker = `# ${section}`
  const endMarker = `# ${section} end`
  const sections: FoundSection[] = []
  let lastStart: { innerStart: number, lineStart: number } | null = null
  let offset = 0

  const lines = content.split('\n')
  for (let lineIndex = 0; lineIndex < lines.length; lineIndex++) {
    const line = lines[lineIndex]
    const lineStart = offset
    const lineLengthWithNewline = lineIndex < lines.length - 1 ? line.length + 1 : line.length
    offset += lineLengthWithNewline

    const trimmed = line.replace(/[\r \t]+$/, '')
    if (trimmed === startMarker) {
      lastStart = { innerStart: offset, lineStart }
    } else if (trimmed === endMarker && lastStart) {
      sections.push(buildFoundSection(content, line, lineStart, lastStart))
      lastStart = null
    }
  }
  return sections
}

function buildFoundSection (
  content: string,
  line: string,
  lineStart: number,
  lastStart: { innerStart: number, lineStart: number }
): FoundSection {
  const { lineStart: startOffset, innerStart } = lastStart
  const inner = content.slice(innerStart, lineStart).replace(/[\r\n]+$/, '')
  const markerLen = line.replace(/[\r\n]+$/, '').length
  const rangeEnd = lineStart + markerLen
  return {
    end: rangeEnd,
    fullMatch: content.slice(startOffset, rangeEnd),
    inner,
    start: startOffset,
  }
}

function pickBestSection (sections: FoundSection[], homeVar: string): FoundSection {
  const settings = sections.map(({ inner }) => stripComments(inner))
  const predicates: Array<(text: string) => boolean> = [
    (text) => text.includes('PATH') && text.includes(homeVar),
    (text) => text.includes(homeVar),
    (text) => text.includes('PATH'),
  ]
  for (const predicate of predicates) {
    for (let sectionIndex = sections.length - 1; sectionIndex >= 0; sectionIndex--) {
      if (predicate(settings[sectionIndex])) return sections[sectionIndex]
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
