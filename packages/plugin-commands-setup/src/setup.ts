import fs from 'fs'
import os from 'os'
import path from 'path'
import { docsUrl } from '@pnpm/cli-utils'
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

export async function handler (
  opts: {
    pnpmHomeDir: string
  }
) {
  const currentShell = process.env.SHELL ? path.basename(process.env.SHELL) : null
  switch (currentShell) {
  case 'bash': {
    const configFile = path.join(os.homedir(), '.bashrc')
    return setupShell(configFile, opts.pnpmHomeDir)
  }
  case 'zsh': {
    const configFile = path.join(os.homedir(), '.zshrc')
    return setupShell(configFile, opts.pnpmHomeDir)
  }
  case 'fish': {
    return setupFishShell(opts.pnpmHomeDir)
  }
  }
  return 'Could not infer shell type.'
}

async function setupShell (configFile: string, pnpmHomeDir: string) {
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

async function setupFishShell (pnpmHomeDir: string) {
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
