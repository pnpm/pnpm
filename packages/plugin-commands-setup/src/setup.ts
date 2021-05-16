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

export async function handler () {
  const bashRC = path.join(os.homedir(), '.bashrc')
  if (!fs.existsSync(bashRC)) return 'Could not setup pnpm. No ~/.bashrc found'
  const bashRCContent = await fs.promises.readFile(bashRC, 'utf8')
  const pnpmHome = getPnpmHome()
  await fs.promises.writeFile(bashRC, `${bashRCContent}
export PNPM_HOME="${pnpmHome}"
export PATH="$PNPM_HOME:$PATH"
`, 'utf8')
  return ''
}

function getPnpmHome () {
  if (process['pkg'] != null) {
    // If the pnpm CLI was bundled by vercel/pkg then we cannot use the js path for npm_execpath
    // because in that case the js is in a virtual filesystem inside the executor.
    // Instead, we use the path to the exe file.
    return path.dirname(process.execPath)
  } else {
    return (require.main != null) ? path.dirname(require.main.filename) : process.cwd()
  }
}
