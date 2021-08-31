import fs from 'fs'
import os from 'os'
import path from 'path'
import execa from 'execa'
import { docsUrl } from '@pnpm/cli-utils'
import logger from '@pnpm/logger'
import renderHelp from 'render-help'

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
  const currentShell = process.env.SHELL ? path.basename(process.env.SHELL) : null
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
    return setupEnvironmentPath(pnpmHomeDir)
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

async function setupEnvironmentPath (pnpmHomeDir: string): Promise<string> {
  const pathRegex = /^ {4}(?<name>PATH) {4}(?<type>\w+) {4}(?<data>.*)$/gim
  const pnpmHomeRegex = /^ {4}(?<name>PNPM_HOME) {4}(?<type>\w+) {4}(?<data>.*)$/gim
  const regKey = 'HKEY_CURRENT_USER\\Environment'

  const queryResult = await execa('reg', ['query', regKey])

  if (queryResult.failed) {
    return 'Win32 registry environment values could not be retrieved'
  }

  const queryOutput = queryResult.stdout
  const pathValueMatch = [...queryOutput.matchAll(pathRegex)]
  const homeValueMatch = [...queryOutput.matchAll(pnpmHomeRegex)]

  const logger = []
  if (homeValueMatch?.length === 1) {
    logger.push(`Currently 'PNPM_HOME' is set to '${homeValueMatch[0]?.groups?.data ?? ''}'`)
  } else {
    logger.push(`Setting 'PNPM_HOME' to value '${pnpmHomeDir}'`)
    const addResult = await execa('reg', ['add', regKey, '/v', 'PNPM_HOME', '/t', 'REG_EXPAND_SZ', '/d', pnpmHomeDir, '/f'])
    if (addResult.failed) {
      logger.push(`\t${addResult.stderr}`)
    } else {
      logger.push(`\t${addResult.stdout}`)
    }
  }

  if (pathValueMatch?.length === 1) {
    const pathData = pathValueMatch[0]?.groups?.data ?? ''
    const pathDataUpperCase = pathData.toUpperCase()
    if (pathDataUpperCase.includes('%PNPM_HOME%')) {
      logger.push('PATH already contains PNPM_HOME')
    } else {
      logger.push('Updating PATH')
      const addResult = await execa('reg', ['add', regKey, '/v', pathValueMatch[0].groups?.name ?? 'PATH', '/t', 'REG_EXPAND_SZ', '/d', `${pathData}%PNPM_HOME%;`, '/f'])
      if (addResult.failed) {
        logger.push(`\t${addResult.stderr}`)
      } else {
        logger.push(`\t${addResult.stdout}`)
      }
    }
  }

  return logger.join('\n')
}
