import fs from 'fs'
import path from 'path'
import { docsUrl } from '@pnpm/cli-utils'
import logger from '@pnpm/logger'
import {
  addDirToEnvPath,
  ConfigReport,
  PathExtenderReport,
} from '@pnpm/os.env.path-extender'
import renderHelp from 'render-help'

export const rcOptionsTypes = () => ({})

export const cliOptionsTypes = () => ({
  force: Boolean,
})

export const shorthands = {}

export const commandNames = ['setup']

export function help () {
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

function getExecPath () {
  if (process['pkg'] != null) {
    // If the pnpm CLI was bundled by vercel/pkg then we cannot use the js path for npm_execpath
    // because in that case the js is in a virtual filesystem inside the executor.
    // Instead, we use the path to the exe file.
    return process.execPath
  }
  return (require.main != null) ? require.main.filename : process.cwd()
}

function copyCli (currentLocation: string, targetDir: string) {
  const newExecPath = path.join(targetDir, path.basename(currentLocation))
  if (path.relative(newExecPath, currentLocation) === '') return
  logger.info({
    message: `Copying pnpm CLI from ${currentLocation} to ${newExecPath}`,
    prefix: process.cwd(),
  })
  fs.mkdirSync(targetDir, { recursive: true })
  fs.copyFileSync(currentLocation, newExecPath)
}

export async function handler (
  opts: {
    force?: boolean
    pnpmHomeDir: string
  }
) {
  const execPath = getExecPath()
  if (execPath.match(/\.[cm]?js$/) == null) {
    copyCli(execPath, opts.pnpmHomeDir)
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

function renderSetupOutput (report: PathExtenderReport) {
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
