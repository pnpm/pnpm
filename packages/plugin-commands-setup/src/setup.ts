import fs from 'fs'
import os from 'os'
import path from 'path'
import { docsUrl } from '@pnpm/cli-utils'
import logger from '@pnpm/logger'
import renderHelp from 'render-help'
import { setupWindowsEnvironmentPath } from './setupOnWindows'
import { BadShellSectionError } from './errors'

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
  const currentShell = detectCurrentShell()
  const updateOutput = await updateShell(currentShell, opts.pnpmHomeDir, { force: opts.force ?? false })
  return `${updateOutput}

Setup complete. Open a new terminal to start using pnpm.`
}

function detectCurrentShell () {
  if (process.env.ZSH_VERSION) return 'zsh'
  if (process.env.BASH_VERSION) return 'bash'
  if (process.env.FISH_VERSION) return 'fish'
  return typeof process.env.SHELL === 'string' ? path.basename(process.env.SHELL) : null
}

async function updateShell (
  currentShell: string | null,
  pnpmHomeDir: string,
  opts: { force: boolean }
): Promise<string> {
  switch (currentShell) {
  case 'bash':
  case 'zsh': {
    return reportShellChange(await setupShell(currentShell, pnpmHomeDir, opts))
  }
  case 'fish': {
    return reportShellChange(await setupFishShell(pnpmHomeDir, opts))
  }
  }

  if (process.platform === 'win32') {
    return setupWindowsEnvironmentPath(pnpmHomeDir, opts)
  }

  return 'Could not infer shell type.'
}

function reportShellChange ({ action, configFile }: ShellSetupResult): string {
  switch (action) {
  case 'created': return `Created ${configFile}`
  case 'added': return `Appended new lines to ${configFile}`
  case 'updated': return `Replaced configuration in ${configFile}`
  case 'skipped': return `Configuration already up-to-date in ${configFile}`
  }
}

type ShellSetupAction = 'created' | 'added' | 'updated' | 'skipped'

interface ShellSetupResult {
  configFile: string
  action: ShellSetupAction
}

async function setupShell (shell: 'bash' | 'zsh', pnpmHomeDir: string, opts: { force: boolean }): Promise<ShellSetupResult> {
  const configFile = path.join(os.homedir(), `.${shell}rc`)
  const content = `# pnpm
export PNPM_HOME="${pnpmHomeDir}"
export PATH="$PNPM_HOME:$PATH"
# pnpm end`
  return {
    action: await updateShellConfig(configFile, content, opts),
    configFile,
  }
}

async function setupFishShell (pnpmHomeDir: string, opts: { force: boolean }): Promise<ShellSetupResult> {
  const configFile = path.join(os.homedir(), '.config/fish/config.fish')
  const content = `# pnpm
set -gx PNPM_HOME "${pnpmHomeDir}"
set -gx PATH "$PNPM_HOME" $PATH
# pnpm end`
  return {
    action: await updateShellConfig(configFile, content, opts),
    configFile,
  }
}

async function updateShellConfig (
  configFile: string,
  newContent: string,
  opts: { force: boolean }
): Promise<ShellSetupAction> {
  if (!fs.existsSync(configFile)) {
    await fs.promises.writeFile(configFile, newContent, 'utf8')
    return 'created'
  }
  const configContent = await fs.promises.readFile(configFile, 'utf8')
  const match = configContent.match(/# pnpm[\s\S]*# pnpm end/)
  if (!match) {
    await fs.promises.appendFile(configFile, `\n${newContent}`, 'utf8')
    return 'added'
  }
  if (match[0] !== newContent) {
    if (!opts.force) {
      throw new BadShellSectionError({ current: match[1], wanted: newContent, configFile })
    }
    const newConfigContent = replaceSection(configContent, newContent)
    await fs.promises.writeFile(configFile, newConfigContent, 'utf8')
    return 'updated'
  }
  return 'skipped'
}

function replaceSection (originalContent: string, newSection: string): string {
  return originalContent.replace(/# pnpm[\s\S]*# pnpm end/g, newSection)
}
