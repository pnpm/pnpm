import fs from 'fs'
import path from 'path'
import { docsUrl } from '@pnpm/cli-utils'
import fetch from '@pnpm/fetch'
import execa from 'execa'
import PATH from 'path-name'
import renderHelp from 'render-help'
import loadJsonFile from 'load-json-file'
import writeJsonFile from 'write-json-file'

export const rcOptionsTypes = () => ({})

export const cliOptionsTypes = () => ({})

export const shorthands = {}

export const commandNames = ['node']

export function help () {
  return renderHelp({
    description: 'Run Node.js',
    descriptionLists: [],
    url: docsUrl('node'),
    usages: ['pnpm node'],
  })
}

export async function handler (
  opts: {
    argv: {
      original: string[]
    }
    nodeVersion?: string
  }
) {
  const nodeDir = await getActiveNodeDir(opts.nodeVersion)
  const result = await execa('node', opts.argv.original.slice(1), {
    env: {
      [PATH]: `${path.join(nodeDir, 'node_modules/.bin')}${path.delimiter}${process.env[PATH]!}`,
    },
    stdout: 'inherit',
    stdin: 'inherit',
  })
  process.exit(result.exitCode)
}

async function getActiveNodeDir (nodeVersion?: string) {
  const pnpmHome = getPnpmHome()
  const nodesDir = path.join(pnpmHome, 'nodes')
  let wantedNodeVersion = nodeVersion ?? (await readNodeVersionsManifest(nodesDir))?.default
  if (wantedNodeVersion == null) {
    await fs.promises.mkdir(nodesDir, { recursive: true })
    const response = await fetch('https://registry.npmjs.org/node')
    wantedNodeVersion = (await response.json())['dist-tags'].lts
    if (wantedNodeVersion == null) {
      throw new Error('Could not resolve LTS version of Node.js')
    }
    await writeJsonFile(path.join(nodesDir, 'versions.json'), {
      default: wantedNodeVersion,
    })
  }
  const versionDir = path.join(nodesDir, wantedNodeVersion)
  if (!fs.existsSync(versionDir)) {
    await fs.promises.mkdir(versionDir, { recursive: true })
    await writeJsonFile(path.join(versionDir, 'package.json'), {})
    const platform = process.platform === 'win32' ? 'win' : process.platform
    const arch = platform === 'win' && process.arch === 'ia32' ? 'x86' : process.arch
    await execa('pnpm', ['add', `node-${platform}-${arch}@${wantedNodeVersion}`], {
      cwd: versionDir,
      stdout: 'inherit',
    })
  }
  return versionDir
}

async function readNodeVersionsManifest (nodesDir: string): Promise<{ default?: string }> {
  try {
    return await loadJsonFile<{ default?: string }>(path.join(nodesDir, 'versions.json'))
  } catch (err) {
    if (err.code === 'ENOENT') {
      return {}
    }
    throw err
  }
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
