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
  const bashRC = path.join(os.homedir(), '.bashrc')
  if (!fs.existsSync(bashRC)) return 'Could not setup pnpm. No ~/.bashrc found'
  const bashRCContent = await fs.promises.readFile(bashRC, 'utf8')
  if (bashRCContent.includes('PNPM_HOME')) return ''
  await fs.promises.writeFile(bashRC, `${bashRCContent}
export PNPM_HOME="${opts.pnpmHomeDir}"
export PATH="$PNPM_HOME:$PATH"
`, 'utf8')
  return ''
}
