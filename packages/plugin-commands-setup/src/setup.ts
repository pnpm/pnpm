import fs from 'fs'
import os from 'os'
import path from 'path'
import { docsUrl } from '@pnpm/cli-utils'
import logger from '@pnpm/logger'
import renderHelp from 'render-help'
import { setupWindowsEnvironmentPath } from './setupOnWindows'

export const rcOptionsTypes = () => ({})

export const cliOptionsTypes = () => ({})

export const shorthands = {}

export const commandNames = ['setup']

export function help () {
  return renderHelp({
    description: 'Sets up pnpm',
    descriptionLists: [
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
    pnpmHomeDir: string
  }
) {
  const currentShell = typeof process.env.SHELL === 'string' ? path.basename(process.env.SHELL) : null
  const execPath = getExecPath()
  if (execPath.match(/\.[cm]?js$/) == null) {
    copyCli(execPath, opts.pnpmHomeDir)
  }
  const updateOutput = await updateShell(currentShell, opts.pnpmHomeDir)
  return `${updateOutput}

Setup complete. Open a new terminal to start using pnpm.`
}

async function updateShell (currentShell: string | null, pnpmHomeDir: string): Promise<string> {
  switch (currentShell) {
  case 'bash': {
    const configFile = path.join(os.homedir(), '.bashrc')
    return setupShell(configFile, pnpmHomeDir)
  }
  case 'zsh': {
    const configFile = path.join(os.homedir(), '.zshrc')
    return setupShell(configFile, pnpmHomeDir)
  }
  case 'fish': {
    return setupFishShell(pnpmHomeDir)
  }
  }

  if (process.platform === 'win32') {
    return setupWindowsEnvironmentPath(pnpmHomeDir)
  }

  return 'Could not infer shell type.'
}

async function setupShell (configFile: string, pnpmHomeDir: string): Promise<string> {
  if (!fs.existsSync(configFile)) return `Could not setup pnpm. No ${configFile} found`
  const configContent = await fs.promises.readFile(configFile, 'utf8')
  if (configContent.includes('PNPM_HOME')) {
    return `PNPM_HOME is already in ${configFile}`
  }
  await fs.promises.writeFile(configFile, `${configContent}
export PNPM_HOME="${pnpmHomeDir}"
export PATH="$PNPM_HOME:$PATH"
`, 'utf8')
  return `Updated ${configFile}`
}

async function setupFishShell (pnpmHomeDir: string): Promise<string> {
  const configFile = path.join(os.homedir(), '.config/fish/config.fish')
  if (!fs.existsSync(configFile)) return `Could not setup pnpm. No ${configFile} found`
  const configContent = await fs.promises.readFile(configFile, 'utf8')
  if (configContent.includes('PNPM_HOME')) {
    return `PNPM_HOME is already in ${configFile}`
  }
  await fs.promises.writeFile(configFile, `${configContent}
set -gx PNPM_HOME "${pnpmHomeDir}"
set -gx PATH "$PNPM_HOME" $PATH
`, 'utf8')
  return `Updated ${configFile}`
}
