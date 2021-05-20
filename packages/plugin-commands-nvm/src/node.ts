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
    useNodeVersion?: string
    pnpmHomeDir: string
  }
) {
  const nodeDir = await getNodeDir(opts.pnpmHomeDir, opts.useNodeVersion)
  const { exitCode } = await execa('node', opts.argv.original.slice(1), {
    env: {
      [PATH]: `${path.join(nodeDir, 'node_modules/.bin')}${path.delimiter}${process.env[PATH]!}`,
    },
    stdout: 'inherit',
    stdin: 'inherit',
  })
  return { exitCode }
}

export async function getNodeDir (pnpmHomeDir: string, nodeVersion?: string) {
  const nodesDir = path.join(pnpmHomeDir, 'nodes')
  let wantedNodeVersion = nodeVersion ?? (await readNodeVersionsManifest(nodesDir))?.default
  await fs.promises.mkdir(nodesDir, { recursive: true })
  fs.writeFileSync(path.join(nodesDir, 'pnpm-workspace.yaml'), '', 'utf8')
  if (wantedNodeVersion == null) {
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
    await installNode(wantedNodeVersion, versionDir)
  }
  return versionDir
}

async function installNode (wantedNodeVersion: string, versionDir: string) {
  await fs.promises.mkdir(versionDir, { recursive: true })
  await writeJsonFile(path.join(versionDir, 'package.json'), {})
  const { exitCode } = await execa('pnpm', ['add', '--use-stderr', `${getNodePkgName()}@${wantedNodeVersion}`], {
    cwd: versionDir,
    stdout: 'inherit',
  })
  if (exitCode !== 0) {
    throw new Error(`Couldn't install Node.js ${wantedNodeVersion}`)
  }
}

function getNodePkgName () {
  const platform = process.platform === 'win32' ? 'win' : process.platform
  const arch = platform === 'win' && process.arch === 'ia32' ? 'x86' : process.arch
  return `node-${platform}-${arch}`
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
