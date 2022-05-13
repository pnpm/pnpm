import fs from 'fs'
import os from 'os'
import path from 'path'
import { docsUrl } from '@pnpm/cli-utils'
import logger from '@pnpm/logger'
import renderHelp from 'render-help'
import { setupWindowsEnvironmentPath } from './setupOnWindows'
import { BadHomeDirError } from './BadHomeDirError'

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

interface ShellSetupResult {
  configFile: string
  action: 'created' | 'added' | 'updated' | 'skipped'
}

async function setupShell (shell: 'bash' | 'zsh', pnpmHomeDir: string, opts: { force: boolean }): Promise<ShellSetupResult> {
  const configFile = path.join(os.homedir(), `.${shell}rc`)
  const content = `# pnpm
export PNPM_HOME="${pnpmHomeDir}"
export PATH="$PNPM_HOME:$PATH"
# pnpm end
`
  if (!fs.existsSync(configFile)) {
    await fs.promises.writeFile(configFile, content, 'utf8')
    return { action: 'created', configFile }
  }
  const configContent = await fs.promises.readFile(configFile, 'utf8')
  if (!configContent.includes('PNPM_HOME')) {
    await fs.promises.appendFile(configFile, `\n${content}`, 'utf8')
    return { action: 'added', configFile }
  }
  const match = configContent.match(/export PNPM_HOME="(.*)"/)
  if (match && match[1] !== pnpmHomeDir) {
    if (!opts.force) {
      throw new BadHomeDirError({ currentDir: match[1], wantedDir: pnpmHomeDir })
    }
    const newConfigContent = replaceSection(configContent, content)
    await fs.promises.writeFile(configFile, newConfigContent, 'utf8')
    return { action: 'updated', configFile }
  }
  return { action: 'skipped', configFile }
}

async function setupFishShell (pnpmHomeDir: string, opts: { force: boolean }): Promise<ShellSetupResult> {
  const configFile = path.join(os.homedir(), '.config/fish/config.fish')
  const content = `# pnpm
set -gx PNPM_HOME "${pnpmHomeDir}"
set -gx PATH "$PNPM_HOME" $PATH
# pnpm end
`
  if (!fs.existsSync(configFile)) {
    await fs.promises.writeFile(configFile, content, 'utf8')
    return { action: 'created', configFile }
  }
  const configContent = await fs.promises.readFile(configFile, 'utf8')
  if (!configContent.includes('PNPM_HOME')) {
    await fs.promises.appendFile(configFile, `\n${content}`, 'utf8')
    return { action: 'added', configFile }
  }
  const match = configContent.match(/set -gx PNPM_HOME "(.*)"/)
  if (match && match[1] !== pnpmHomeDir) {
    if (!opts.force) {
      throw new BadHomeDirError({ currentDir: match[1], wantedDir: pnpmHomeDir })
    }
    const newConfigContent = replaceSection(configContent, content)
    await fs.promises.writeFile(configFile, newConfigContent, 'utf8')
    return { action: 'updated', configFile }
  }
  return { action: 'skipped', configFile }
}

function replaceSection (originalContent: string, newSection: string): string {
  return originalContent.replace(/# pnpm[\s\S]*# pnpm end/g, newSection)
}
