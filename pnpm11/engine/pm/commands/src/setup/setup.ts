import { spawnSync } from 'node:child_process'
import fs from 'node:fs'
import path from 'node:path'
import util from 'node:util'

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

function validateGitHubActionsEnvironmentFileValues (pnpmHomeDir: string, binDir: string): void {
  if (!shouldPersistGitHubActionsEnvironmentFiles()) return
  validateGitHubActionsEnvironmentFileValue('PNPM_HOME', pnpmHomeDir)
  validateGitHubActionsEnvironmentFileValue('pnpm setup bin directory', binDir)
}

/**
 * `GITHUB_ENV` and `GITHUB_PATH` are line-oriented, so a line break in a
 * persisted value would append attacker-chosen records to the environment of
 * every later step in the workflow job.
 */
function validateGitHubActionsEnvironmentFileValue (name: string, value: string): void {
  if (value.includes('\n') || value.includes('\r') || value.includes('\0')) {
    throw new PnpmError('BAD_GITHUB_ACTIONS_ENVIRONMENT_VALUE', `${name} cannot contain newline or NUL characters`)
  }
}

function writeGitHubActionsEnvironmentFiles (pnpmHomeDir: string, binDir: string): void {
  if (!shouldPersistGitHubActionsEnvironmentFiles()) return
  const githubEnv = process.env.GITHUB_ENV
  const githubPath = process.env.GITHUB_PATH
  if (githubEnv != null) {
    appendGitHubActionsEnvironmentFile('GITHUB_ENV', githubEnv, `PNPM_HOME=${pnpmHomeDir}`)
  }
  if (githubPath != null) {
    appendGitHubActionsEnvironmentFile('GITHUB_PATH', githubPath, binDir)
  }
}

function shouldPersistGitHubActionsEnvironmentFiles (): boolean {
  return process.env.GITHUB_ACTIONS === 'true' && (process.env.GITHUB_ENV != null || process.env.GITHUB_PATH != null)
}

/**
 * The runner creates both files up front, so anything but an existing regular
 * file at `filePath` is not the runner's target: skip it instead of creating
 * it or following a symlink to it. A failure on one target must not stop the
 * other from being written.
 */
function appendGitHubActionsEnvironmentFile (targetName: string, filePath: string, line: string): void {
  try {
    if (!fs.lstatSync(filePath).isFile()) return
    appendLineToRegularFile(filePath, line)
  } catch (err: unknown) {
    if (util.types.isNativeError(err) && (err as NodeJS.ErrnoException).code === 'ENOENT') return
    logger.warn({
      message: `Failed to write GitHub Actions environment file ${targetName} (${filePath}): ${util.types.isNativeError(err) ? err.message : String(err)}`,
      prefix: process.cwd(),
    })
  }
}

function appendLineToRegularFile (filePath: string, line: string): void {
  const fd = fs.openSync(
    filePath,
    fs.constants.O_RDWR |
      fs.constants.O_APPEND |
      (process.platform === 'win32' ? 0 : fs.constants.O_NOFOLLOW)
  )
  try {
    const stats = fs.fstatSync(fd)
    if (!stats.isFile()) return
    fs.writeSync(fd, `${missingRecordSeparator(fd, stats.size)}${line}\n`, null, 'utf8')
  } finally {
    fs.closeSync(fd)
  }
}

function missingRecordSeparator (fd: number, size: number): string {
  if (size === 0) return ''
  const lastByte = Buffer.allocUnsafe(1)
  const bytesRead = fs.readSync(fd, lastByte, 0, 1, size - 1)
  return bytesRead === 1 && lastByte[0] !== 0x0A ? '\n' : ''
}

// v10-layout shim names that v11 writes under pnpmHomeDir/bin instead.
export const LEGACY_HOME_DIR_SHIM_NAMES = [
  'pnpm', 'pnpm.cmd', 'pnpm.ps1',
  'pn', 'pn.cmd', 'pn.ps1',
  'pnpx', 'pnpx.cmd', 'pnpx.ps1',
  'pnx', 'pnx.cmd', 'pnx.ps1',
]

function removeLegacyHomeDirShims (pnpmHomeDir: string): void {
  for (const name of LEGACY_HOME_DIR_SHIM_NAMES) {
    try {
      fs.rmSync(path.join(pnpmHomeDir, name), { force: true })
    } catch {}
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
  validateGitHubActionsEnvironmentFileValues(opts.pnpmHomeDir, binDir)
  if (execPath.match(/\.[cm]?js$/) == null) {
    installCliGlobally(execPath, opts.pnpmHomeDir)
    createAliasScripts(binDir)
  }
  try {
    const report = await addDirToEnvPath(opts.pnpmHomeDir, {
      configSectionName: 'pnpm',
      proxyVarName: 'PNPM_HOME',
      proxyVarSubDir: 'bin',
      overwrite: opts.force,
      position: 'start',
    })
    writeGitHubActionsEnvironmentFiles(opts.pnpmHomeDir, binDir)
    removeLegacyHomeDirShims(opts.pnpmHomeDir)
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
source ${report.configFile.path}
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
