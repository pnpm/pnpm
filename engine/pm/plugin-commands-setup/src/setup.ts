import { spawnSync } from 'node:child_process'
import fs from 'node:fs'
import path from 'node:path'

import { detectIfCurrentPkgIsExecutable, packageManager } from '@pnpm/cli-meta'
import { docsUrl } from '@pnpm/cli-utils'
import { logger } from '@pnpm/logger'
import {
  addDirToEnvPath,
  type ConfigReport,
  type PathExtenderReport,
} from '@pnpm/os.env.path-extender'
import { renderHelp } from 'render-help'

export const rcOptionsTypes = (): Record<string, unknown> => ({})

export const cliOptionsTypes = (): Record<string, unknown> => ({
  force: Boolean,
})

export const shorthands = {}

export const commandNames = ['setup']

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
      bin: { pnpm: execName },
    }))
    createdPkgJson = true
  }

  logger.info({
    message: `Installing pnpm CLI globally from ${execDir}`,
    prefix: process.cwd(),
  })

  try {
    const { status, error } = spawnSync(execPath, ['add', '-g', `file:${execDir}`], {
      stdio: 'inherit',
      env: {
        ...process.env,
        PNPM_HOME: pnpmHomeDir,
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

function createPnpxScripts (targetDir: string): void {
  // Why script files instead of aliases?
  // 1. Aliases wouldn't work on all platform, such as Windows Command Prompt or POSIX `sh`.
  // 2. Aliases wouldn't work on all environments, such as non-interactive shells and CI environments.
  // 3. Aliases must be set for different shells while script files are limited to only 2 types: POSIX and Windows.
  // 4. Aliases cannot be located with the `which` or `where` command.
  // 5. Editing rc files is more error-prone than just write new files to the filesystem.

  fs.mkdirSync(targetDir, { recursive: true })

  // windows can also use shell script via mingw or cygwin so no filter
  const shellScript = [
    '#!/bin/sh',
    'exec pnpm dlx "$@"',
  ].join('\n')
  fs.writeFileSync(path.join(targetDir, 'pnpx'), shellScript, { mode: 0o755 })

  if (process.platform === 'win32') {
    const batchScript = [
      '@echo off',
      'pnpm dlx %*',
    ].join('\n')
    fs.writeFileSync(path.join(targetDir, 'pnpx.cmd'), batchScript)

    const powershellScript = 'pnpm dlx @args'
    fs.writeFileSync(path.join(targetDir, 'pnpx.ps1'), powershellScript)
  }
}

export async function handler (
  opts: {
    force?: boolean
    pnpmHomeDir: string
  }
): Promise<string> {
  const execPath = getExecPath()
  if (execPath.match(/\.[cm]?js$/) == null) {
    installCliGlobally(execPath, opts.pnpmHomeDir)
    createPnpxScripts(opts.pnpmHomeDir)
  }
  try {
    const report = await addDirToEnvPath(opts.pnpmHomeDir, {
      configSectionName: 'pnpm',
      proxyVarName: 'PNPM_HOME',
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
