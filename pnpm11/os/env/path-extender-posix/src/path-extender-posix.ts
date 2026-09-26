import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { PnpmError } from '@pnpm/error'

class BadShellSectionError extends PnpmError {
  public current: string
  public wanted: string
  constructor (opts: { configSectionName: string, wanted: string, current: string, configFile: string }) {
    super('BAD_SHELL_SECTION', `The config file at "${opts.configFile} already contains a ${opts.configSectionName} section but with other configuration`)
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

function detectCurrentShell () {
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
  if (!currentShell) throw new PnpmError('UNKNOWN_SHELL', 'Could not infer shell type.', {
    hint: `Set the SHELL environment variable to your active shell.
${supportedShellsMsg}`,
  })
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
  let newSettings!: string
  const _createPathValue = createPathValue.bind(null, opts.position ?? 'start')
  if (opts.proxyVarName) {
    const pathRef = opts.proxyVarSubDir ? `$${opts.proxyVarName}/${opts.proxyVarSubDir}` : `$${opts.proxyVarName}`
    newSettings = `export ${opts.proxyVarName}="${dir}"
case ":$PATH:" in
  *":${pathRef}:"*) ;;
  *) export PATH="${_createPathValue(pathRef)}" ;;
esac`
  } else {
    newSettings = `case ":$PATH:" in
  *":${dir}:"*) ;;
  *) export PATH="${_createPathValue(dir)}" ;;
esac`
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

function createPathValue (position: AddingPosition, dir: string) {
  return position === 'start'
    ? `${dir}:$PATH`
    : `$PATH:${dir}`
}

async function setupFishShell (dir: string, opts: AddDirToPosixEnvPathOpts): Promise<PathExtenderPosixReport> {
  const configFile = path.join(os.homedir(), '.config/fish/config.fish')
  let newSettings!: string
  const _createPathValue = createFishPathValue.bind(null, opts.position ?? 'start')
  if (opts.proxyVarName) {
    const pathRef = opts.proxyVarSubDir ? `$${opts.proxyVarName}/${opts.proxyVarSubDir}` : `$${opts.proxyVarName}`
    const matchPattern = opts.proxyVarSubDir ? `"${pathRef}"` : pathRef
    newSettings = `set -gx ${opts.proxyVarName} "${dir}"
if not string match -q -- ${matchPattern} $PATH
  set -gx PATH ${_createPathValue(pathRef)}
end`
  } else {
    newSettings = `if not string match -q -- "${dir}" $PATH
  set -gx PATH ${_createPathValue(dir)}
end`
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
  let newSettings!: string
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
function wrapSettings (sectionName: string, settings: string): string {
  return `# ${sectionName}
${settings}
# ${sectionName} end`
}

function createFishPathValue (position: AddingPosition, dir: string) {
  return position === 'start'
    ? `"${dir}" $PATH`
    : `$PATH "${dir}"`
}

interface UpdateShellResult {
  changeType: ConfigFileChangeType
  oldSettings: string
}

async function updateShellConfig (
  configFile: string,
  newContent: string,
  opts: AddDirToPosixEnvPathOpts
): Promise<UpdateShellResult> {
  if (!fs.existsSync(configFile)) {
    await fs.promises.mkdir(path.dirname(configFile), { recursive: true })
    await fs.promises.writeFile(configFile, `${newContent}\n`, 'utf8')
    return {
      changeType: 'created',
      oldSettings: '',
    }
  }
  const configContent = await fs.promises.readFile(configFile, 'utf8')
  const match = new RegExp(`# ${opts.configSectionName}\n([\\s\\S]*)\n# ${opts.configSectionName} end`, 'g').exec(configContent)
  if (!match) {
    await fs.promises.appendFile(configFile, `\n${newContent}\n`, 'utf8')
    return {
      changeType: 'appended',
      oldSettings: '',
    }
  }
  const oldSettings = match[1]
  if (match[0] !== newContent) {
    if (!opts.overwrite) {
      throw new BadShellSectionError({
        configSectionName: opts.configSectionName,
        current: match[0],
        wanted: newContent,
        configFile,
      })
    }
    const newConfigContent = replaceSection(configContent, newContent, opts.configSectionName)
    await fs.promises.writeFile(configFile, newConfigContent, 'utf8')
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

function replaceSection (originalContent: string, newSection: string, sectionName: string): string {
  return originalContent.replace(new RegExp(`# ${sectionName}[\\s\\S]*# ${sectionName} end`, 'g'), newSection)
}
