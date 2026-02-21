import fs from 'fs'
import path from 'path'
import { detectIfCurrentPkgIsExecutable, packageManager } from '@pnpm/cli-meta'
import { docsUrl } from '@pnpm/cli-utils'
import { logger } from '@pnpm/logger'
import {
  addDirToEnvPath,
  type ConfigReport,
  type PathExtenderReport,
} from '@pnpm/os.env.path-extender'
import renderHelp from 'render-help'
import rimraf from '@zkochan/rimraf'
import cmdShim from '@zkochan/cmd-shim'

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
 * Copy the CLI into a directory on the PATH and create a command shim to run it.
 * Without the shim, `pnpm self-update` on Windows cannot replace the running executable
 * and fails with: `EPERM: operation not permitted, unlink 'C:\Users\<user>\AppData\Local\pnpm\pnpm.exe'`.
 * Related issue: https://github.com/pnpm/pnpm/issues/5700
 */
async function copyCli (currentLocation: string, targetDir: string): Promise<void> {
  const toolsDir = path.join(targetDir, '.tools/pnpm-exe', packageManager.version)
  const newExecPath = path.join(toolsDir, path.basename(currentLocation))
  if (path.relative(newExecPath, currentLocation) === '') return
  logger.info({
    message: `Copying pnpm CLI from ${currentLocation} to ${newExecPath}`,
    prefix: process.cwd(),
  })
  fs.mkdirSync(toolsDir, { recursive: true })
  rimraf.sync(newExecPath)
  fs.copyFileSync(currentLocation, newExecPath)
  // For SEA binaries, also copy the dist/ directory that lives alongside the binary.
  const distSrc = path.join(path.dirname(currentLocation), 'dist')
  if (fs.existsSync(distSrc)) {
    const distDest = path.join(toolsDir, 'dist')
    fs.rmSync(distDest, { recursive: true, force: true })
    fs.cpSync(distSrc, distDest, { recursive: true })
  }
  await cmdShim(newExecPath, path.join(targetDir, 'pnpm'), {
    createPwshFile: false,
  })
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
    await copyCli(execPath, opts.pnpmHomeDir)
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
