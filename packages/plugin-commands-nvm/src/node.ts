import fs from 'fs'
import path from 'path'
import { docsUrl } from '@pnpm/cli-utils'
import { Config } from '@pnpm/config'
import fetch, { createFetchFromRegistry, FetchFromRegistry } from '@pnpm/fetch'
import { createCafsStore } from '@pnpm/package-store'
import storePath from '@pnpm/store-path'
import createFetcher, { waitForFilesIndex } from '@pnpm/tarball-fetcher'
import AdmZip from 'adm-zip'
import execa from 'execa'
import PATH from 'path-name'
import renameOverwrite from 'rename-overwrite'
import renderHelp from 'render-help'
import tempy from 'tempy'
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

export type NvmNodeCommandOptions = Pick<Config,
| 'rawConfig'
| 'fetchRetries'
| 'fetchRetryFactor'
| 'fetchRetryMaxtimeout'
| 'fetchRetryMintimeout'
| 'fetchRetryMintimeout'
| 'fetchTimeout'
| 'userAgent'
| 'ca'
| 'cert'
| 'httpProxy'
| 'httpsProxy'
| 'key'
| 'localAddress'
| 'noProxy'
| 'strictSsl'
| 'storeDir'
| 'useNodeVersion'
| 'pnpmHomeDir'
>

export async function handler (
  opts: NvmNodeCommandOptions & {
    argv: {
      original: string[]
    }
  }
) {
  const nodeDir = await getNodeDir(opts)
  const { exitCode } = await execa('node', opts.argv.original.slice(1), {
    env: {
      [PATH]: `${nodeDir}${path.delimiter}${process.env[PATH]!}`,
    },
    stdout: 'inherit',
    stdin: 'inherit',
  })
  return { exitCode }
}

export async function getNodeDir (opts: NvmNodeCommandOptions) {
  const nodesDir = path.join(opts.pnpmHomeDir, 'nodes')
  let wantedNodeVersion = opts.useNodeVersion ?? (await readNodeVersionsManifest(nodesDir))?.default
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
    await installNode(wantedNodeVersion, versionDir, opts)
  }
  return process.platform === 'win32' ? versionDir : path.join(versionDir, 'bin')
}

async function installNode (wantedNodeVersion: string, versionDir: string, opts: NvmNodeCommandOptions) {
  await fs.promises.mkdir(versionDir, { recursive: true })
  const { tarball, pkgName } = getNodeJSTarball(wantedNodeVersion)
  const fetchFromRegistry = createFetchFromRegistry(opts)
  if (tarball.endsWith('.zip')) {
    await downloadAndUnpackZip(fetchFromRegistry, tarball, versionDir, pkgName)
    return
  }
  const getCredentials = () => ({ authHeaderValue: undefined, alwaysAuth: undefined })
  const fetch = createFetcher(fetchFromRegistry, getCredentials, {
    retry: {
      maxTimeout: opts.fetchRetryMaxtimeout,
      minTimeout: opts.fetchRetryMintimeout,
      retries: opts.fetchRetries,
      factor: opts.fetchRetryFactor,
    },
    timeout: opts.fetchTimeout,
  })
  const storeDir = await storePath(process.cwd(), opts.storeDir)
  const cafsDir = path.join(storeDir, 'files')
  const cafs = createCafsStore(cafsDir)
  const { filesIndex } = await fetch.tarball(cafs, { tarball }, {
    lockfileDir: process.cwd(),
  })
  await cafs.importPackage(versionDir, {
    filesResponse: {
      filesIndex: await waitForFilesIndex(filesIndex),
      fromStore: false,
    },
    force: true,
  })
}

async function downloadAndUnpackZip (
  fetchFromRegistry: FetchFromRegistry,
  zipUrl: string,
  targetDir: string,
  pkgName: string
) {
  const response = await fetchFromRegistry(zipUrl)
  const tmp = path.join(tempy.directory(), 'pnpm.zip')
  const dest = fs.createWriteStream(tmp)
  await new Promise((resolve, reject) => {
    response.body.pipe(dest).on('error', reject).on('close', resolve)
  })
  const zip = new AdmZip(tmp)
  const nodeDir = path.dirname(targetDir)
  zip.extractAllTo(nodeDir, true)
  await renameOverwrite(path.join(nodeDir, pkgName), targetDir)
  await fs.promises.unlink(tmp)
}

function getNodeJSTarball (nodeVersion: string) {
  const platform = process.platform === 'win32' ? 'win' : process.platform
  const arch = platform === 'win' && process.arch === 'ia32' ? 'x86' : process.arch
  const extension = platform === 'win' ? 'zip' : 'tar.gz'
  const pkgName = `node-v${nodeVersion}-${platform}-${arch}`
  return {
    pkgName,
    tarball: `https://nodejs.org/download/release/v${nodeVersion}/${pkgName}.${extension}`,
  }
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
