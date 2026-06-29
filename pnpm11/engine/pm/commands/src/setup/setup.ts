import { spawnSync } from 'node:child_process'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { detectIfCurrentPkgIsExecutable, packageManager } from '@pnpm/cli.meta'
import { docsUrl } from '@pnpm/cli.utils'
import { PnpmError } from '@pnpm/error'
import { logger } from '@pnpm/logger'
import {
  addDirToEnvPath,
  type ConfigReport,
  type PathExtenderReport,
} from '@pnpm/os.env.path-extender'
import PATH from 'path-name'
import { renderHelp } from 'render-help'

export const rcOptionsTypes = (): Record<string, unknown> => ({})

export const cliOptionsTypes = (): Record<string, unknown> => ({
  force: Boolean,
})

export const shorthands = {}

export const commandNames = ['setup']

export const overridableByScript = true

export function help (): string {
  return renderHelp({
    description: 'Sets up pnpm',
    descriptionLists: [
      {
        title: 'Options',

        list: [
          {
            description: 'Override the PNPM_HOME env variable in case it already exists',
            name: '--force',
            shortAlias: '-f',
          },
        ],
      },
    ],
    url: docsUrl('setup'),
    usages: ['pnpm setup'],
  })
}

function getExecPath (): string {
  if (detectIfCurrentPkgIsExecutable()) {
    // If the pnpm CLI is a single executable application (SEA), we use the path
    // to the exe file instead of the js path.
    return process.execPath
  }
  return process.argv[1] ?? process.cwd()
}

/**
 * Install the CLI as a global package using `pnpm add -g file:<dir>`.
 * This places pnpm in the standard global directory alongside other
 * globally installed packages.
 */
function installCliGlobally (execPath: string, pnpmHomeDir: string): void {
  const execDir = path.dirname(execPath)
  const execName = path.basename(execPath)
  const pkgJsonPath = path.join(execDir, 'package.json')

  // Write a package.json if one doesn't already exist.
  // (Updated tarballs on GitHub Pages will ship with package.json already.)
  let createdPkgJson = false
  if (!fs.existsSync(pkgJsonPath)) {
    fs.writeFileSync(pkgJsonPath, JSON.stringify({
      name: '@pnpm/exe',
      version: packageManager.version,
      bin: { pnpm: execName, pn: execName },
    }))
    createdPkgJson = true
  }

  logger.info({
    message: `Installing pnpm CLI globally from ${execDir}`,
    prefix: process.cwd(),
  })

  try {
    const binDir = path.join(pnpmHomeDir, 'bin')
    // @pnpm/exe ships a `preinstall`/`prepare` pair (setup.js/prepare.js) that
    // hardlinks the platform-specific binary out of its optional platform
    // packages. None of that applies here: this `file:` dependency is the
    // standalone executable itself (its binary is already present), the
    // platform packages aren't installed alongside it, and the SEA host may
    // have no `node` to run the scripts at all. Skipping them avoids a build
    // approval prompt for pnpm's own install. See
    // https://github.com/pnpm/pnpm/issues/12377.
    const { status, error } = spawnSync(execPath, ['add', '-g', '--ignore-scripts', `file:${execDir}`], {
      stdio: 'inherit',
      env: {
        ...process.env,
        PNPM_HOME: pnpmHomeDir,
        [PATH]: `${binDir}${path.delimiter}${process.env[PATH] ?? ''}`,
      },
    })

    if (error) throw error
    if (status !== 0) {
      throw new Error(`Failed to install pnpm globally (exit code ${status})`)
    }
  } finally {
    if (createdPkgJson) {
      fs.unlinkSync(pkgJsonPath)
    }
  }
}

function createAliasScripts (targetDir: string): void {
  // Why script files instead of aliases?
  // 1. Aliases wouldn't work on all platform, such as Windows Command Prompt or POSIX `sh`.
  // 2. Aliases wouldn't work on all environments, such as non-interactive shells and CI environments.
  // 3. Aliases must be set for different shells while script files are limited to only 2 types: POSIX and Windows.
  // 4. Aliases cannot be located with the `which` or `where` command.
  // 5. Editing rc files is more error-prone than just write new files to the filesystem.

  fs.mkdirSync(targetDir, { recursive: true })

  createShellScript(targetDir, 'pn', 'pnpm')
  createShellScript(targetDir, 'pnpx', 'pnpm dlx')
  createShellScript(targetDir, 'pnx', 'pnpm dlx')
}

function createShellScript (targetDir: string, name: string, command: string): void {
  // windows can also use shell script via mingw or cygwin so no filter
  const shellScript = `#!/bin/sh\nexec ${command} "$@"\n`
  fs.writeFileSync(path.join(targetDir, name), shellScript, { mode: 0o755 })

  if (process.platform === 'win32') {
    fs.writeFileSync(path.join(targetDir, `${name}.cmd`), `@echo off\n${command} %*\n`)
    fs.writeFileSync(path.join(targetDir, `${name}.ps1`), `${command} @args\n`)
  }
}

export async function handler (
  opts: {
    force?: boolean
    pnpmHomeDir: string
  }
): Promise<string> {
  const execPath = getExecPath()
  const binDir = path.join(opts.pnpmHomeDir, 'bin')
  if (execPath.match(/\.[cm]?js$/) == null) {
    installCliGlobally(execPath, opts.pnpmHomeDir)
    createAliasScripts(binDir)
  }
  try {
    const report = isFishShell()
      ? await setupFishConfDir(opts.pnpmHomeDir, opts)
      : await addDirToEnvPath(opts.pnpmHomeDir, {
        configSectionName: 'pnpm',
        proxyVarName: 'PNPM_HOME',
        proxyVarSubDir: 'bin',
        overwrite: opts.force,
        position: 'start',
      })
    return renderSetupOutput(report)
  } catch (err: any) { // eslint-disable-line
    switch (err.code) {
      case 'ERR_PNPM_BAD_ENV_FOUND':
        err.hint = 'If you want to override the existing env variable, use the --force option'
        break
      case 'ERR_PNPM_BAD_SHELL_SECTION':
        err.hint = 'If you want to override the existing configuration section, use the --force option'
        break
    }
    throw err
  }
}

function isFishShell (): boolean {
  return Boolean(process.env.FISH_VERSION) || path.basename(process.env.SHELL ?? '') === 'fish'
}

async function setupFishConfDir (
  pnpmHomeDir: string,
  opts: {
    force?: boolean
  }
): Promise<PathExtenderReport> {
  const configFile = getFishConfigFile()
  const newSettings = `set -gx PNPM_HOME "${escapeFishString(pnpmHomeDir)}"
if not string match -q -- "$PNPM_HOME/bin" $PATH
  set -gx PATH "$PNPM_HOME/bin" $PATH
end`
  const newContent = `${newSettings}\n`
  await ensureFishConfigDir(configFile)
  const existingConfig = await readExistingFishConfig(configFile)
  if (existingConfig == null) {
    await fs.promises.mkdir(path.dirname(configFile), { recursive: true })
    await writeFileAtomically(configFile, newContent)
    return {
      configFile: {
        path: configFile,
        changeType: 'created',
      },
      oldSettings: '',
      newSettings,
    }
  }

  const oldContent = normalizeLineEndings(existingConfig.content)
  const normalizedNewContent = normalizeLineEndings(newContent)
  const oldSettings = trimTrailingNewline(oldContent)
  const normalizedNewSettings = trimTrailingNewline(normalizedNewContent)
  if (oldContent === normalizedNewContent || oldSettings === normalizedNewSettings) {
    return {
      configFile: {
        path: configFile,
        changeType: 'skipped',
      },
      oldSettings,
      newSettings,
    }
  }
  if (!opts.force) {
    throw new PnpmError('BAD_SHELL_SECTION', `The config file at "${configFile}" already contains a pnpm configuration but with other settings`)
  }
  await writeFileAtomically(configFile, newContent, existingConfig.mode)
  return {
    configFile: {
      path: configFile,
      changeType: 'modified',
    },
    oldSettings,
    newSettings,
  }
}

function getFishConfigFile (): string {
  const configHome = getFishConfigHome()
  return path.join(configHome, 'fish/conf.d/pnpm.fish')
}

function getFishConfigHome (): string {
  if (process.env.XDG_CONFIG_HOME) {
    if (!path.isAbsolute(process.env.XDG_CONFIG_HOME)) {
      throw new PnpmError('UNSAFE_SHELL_CONFIG', 'XDG_CONFIG_HOME must be an absolute path when writing fish configuration')
    }
    if (hasControlCharacter(process.env.XDG_CONFIG_HOME)) {
      throw new PnpmError('UNSAFE_SHELL_CONFIG', 'XDG_CONFIG_HOME cannot contain control characters when writing fish configuration')
    }
    return process.env.XDG_CONFIG_HOME
  }
  const configHome = path.join(os.homedir(), '.config')
  if (!path.isAbsolute(configHome)) {
    throw new PnpmError('UNSAFE_SHELL_CONFIG', 'The home directory must resolve to an absolute path when writing fish configuration')
  }
  return configHome
}

function escapeFishString (value: string): string {
  if (hasControlCharacter(value)) {
    throw new PnpmError('UNSAFE_SHELL_CONFIG', 'PNPM_HOME cannot contain control characters when writing fish configuration')
  }
  if (value.includes(path.delimiter)) {
    throw new PnpmError('UNSAFE_SHELL_CONFIG', 'PNPM_HOME cannot contain the PATH delimiter when writing fish configuration')
  }
  return value
    .replace(/\\/g, '\\\\')
    .replace(/"/g, '\\"')
    .replace(/\$/g, '\\$')
}

function hasControlCharacter (value: string): boolean {
  return [...value].some((character) => {
    const code = character.charCodeAt(0)
    return code <= 0x1F || (code >= 0x7F && code <= 0x9F)
  })
}

async function ensureFishConfigDir (configFile: string): Promise<void> {
  const configHome = path.dirname(path.dirname(path.dirname(configFile)))
  await ensureDirectoryWithoutSymlink(configHome)
  await ensureDirectoryWithoutSymlink(path.join(configHome, 'fish'))
  await ensureDirectoryWithoutSymlink(path.dirname(configFile))
}

async function ensureDirectoryWithoutSymlink (dir: string): Promise<void> {
  let stat: fs.Stats
  try {
    stat = await fs.promises.lstat(dir)
  } catch (err: any) { // eslint-disable-line
    if (err.code !== 'ENOENT') throw err
    await fs.promises.mkdir(dir, { recursive: true })
    stat = await fs.promises.lstat(dir)
  }
  if (stat.isSymbolicLink()) {
    throw new PnpmError('UNSAFE_SHELL_CONFIG', `Refusing to write fish configuration under "${dir}" because it is a symbolic link`)
  }
  if (!stat.isDirectory()) {
    throw new PnpmError('UNSAFE_SHELL_CONFIG', `Refusing to write fish configuration under "${dir}" because it is not a directory`)
  }
}

async function readExistingFishConfig (configFile: string): Promise<{ content: string, mode: number } | undefined> {
  let stat: fs.Stats
  try {
    stat = await fs.promises.lstat(configFile)
  } catch (err: any) { // eslint-disable-line
    if (err.code === 'ENOENT') return undefined
    throw err
  }
  if (stat.isSymbolicLink()) {
    throw new PnpmError('UNSAFE_SHELL_CONFIG', `Refusing to write fish configuration at "${configFile}" because it is a symbolic link`)
  }
  if (!stat.isFile()) {
    throw new PnpmError('UNSAFE_SHELL_CONFIG', `Refusing to write fish configuration at "${configFile}" because it is not a regular file`)
  }
  return {
    content: await fs.promises.readFile(configFile, 'utf8'),
    mode: stat.mode & 0o777,
  }
}

async function writeFileAtomically (filePath: string, content: string, mode = 0o644): Promise<void> {
  const tempDir = await fs.promises.mkdtemp(path.join(path.dirname(filePath), `.${path.basename(filePath)}.`))
  const tempPath = path.join(tempDir, path.basename(filePath))
  try {
    await fs.promises.writeFile(tempPath, content, { encoding: 'utf8', mode })
    await renameReplacingDestination(tempPath, filePath)
  } catch (err: any) { // eslint-disable-line
    throw err
  } finally {
    await fs.promises.rm(tempDir, { force: true, recursive: true }).catch(() => undefined)
  }
}

async function renameReplacingDestination (tempPath: string, filePath: string): Promise<void> {
  try {
    await fs.promises.rename(tempPath, filePath)
  } catch (err: any) { // eslint-disable-line
    if (err.code !== 'EEXIST' && err.code !== 'EPERM') {
      throw err
    }
    await fs.promises.rm(filePath, { force: true })
    await fs.promises.rename(tempPath, filePath)
  }
}

function normalizeLineEndings (content: string): string {
  return content.replace(/\r\n/g, '\n')
}

function trimTrailingNewline (content: string): string {
  return content.endsWith('\n') ? content.slice(0, -1) : content
}

function quoteShellPath (value: string): string {
  if (hasControlCharacter(value)) {
    throw new PnpmError('UNSAFE_SHELL_CONFIG', 'The configuration path cannot contain control characters')
  }
  if (/^[\w@%+=:,./~-]+$/.test(value)) {
    return value
  }
  if (value.startsWith('~/')) {
    return `~/${quoteShellString(value.slice(2))}`
  }
  return quoteShellString(value)
}

function quoteShellString (value: string): string {
  return `"${value
    .replace(/\\/g, '\\\\')
    .replace(/"/g, '\\"')
    .replace(/\$/g, '\\$')
    .replace(/`/g, '\\`')}"`
}

function renderSetupOutput (report: PathExtenderReport): string {
  if (report.oldSettings === report.newSettings) {
    return 'No changes to the environment were made. Everything is already up to date.'
  }
  const output = []
  if (report.configFile) {
    output.push(reportConfigChange(report.configFile))
  }
  output.push(`Next configuration changes were made:
${report.newSettings}`)
  if (report.configFile == null) {
    output.push('Setup complete. Open a new terminal to start using pnpm.')
  } else if (report.configFile.changeType !== 'skipped') {
    output.push(`To start using pnpm, run:
source ${quoteShellPath(report.configFile.path)}
`)
  }
  return output.join('\n\n')
}

function reportConfigChange (configReport: ConfigReport): string {
  switch (configReport.changeType) {
    case 'created': return `Created ${configReport.path}`
    case 'appended': return `Appended new lines to ${configReport.path}`
    case 'modified': return `Replaced configuration in ${configReport.path}`
    case 'skipped': return `Configuration already up to date in ${configReport.path}`
  }
}
