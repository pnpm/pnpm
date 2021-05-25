import fs from 'fs'
import path from 'path'
import { docsUrl } from '@pnpm/cli-utils'
import fetch, { createFetchFromRegistry } from '@pnpm/fetch'
import { PackageFileInfo } from '@pnpm/fetcher-base'
import { createCafsStore } from '@pnpm/package-store'
import storePath from '@pnpm/store-path'
import createFetcher from '@pnpm/tarball-fetcher'
import execa from 'execa'
import PATH from 'path-name'
import R from 'ramda'
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
    storeDir?: string
  }
) {
  const nodeDir = await getNodeDir(opts, opts.pnpmHomeDir, opts.useNodeVersion)
  const { exitCode } = await execa('node', opts.argv.original.slice(1), {
    env: {
      [PATH]: `${path.join(nodeDir, 'bin')}${path.delimiter}${process.env[PATH]!}`,
    },
    stdout: 'inherit',
    stdin: 'inherit',
  })
  return { exitCode }
}

export async function getNodeDir (opts: { storeDir?: string }, pnpmHomeDir: string, nodeVersion?: string) {
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
    await installNode(wantedNodeVersion, versionDir, opts)
  }
  return versionDir
}

async function installNode (wantedNodeVersion: string, versionDir: string, opts: { storeDir?: string }) {
  await fs.promises.mkdir(versionDir, { recursive: true })
  await writeJsonFile(path.join(versionDir, 'package.json'), {})
  const resolution = {
    tarball: getNodeJSTarball(wantedNodeVersion),
  }
  const fetchFromRegistry = createFetchFromRegistry({})
  const getCredentials = () => ({ authHeaderValue: undefined, alwaysAuth: undefined })
  const fetch = createFetcher(fetchFromRegistry, getCredentials, {
    retry: {
      maxTimeout: 100,
      minTimeout: 0,
      retries: 1,
    },
  })
  const storeDir = await storePath(process.cwd(), opts.storeDir)
  const cafsDir = path.join(storeDir, 'files')
  const cafs = createCafsStore(cafsDir)
  const { filesIndex } = await fetch.tarball(cafs, resolution, {
    lockfileDir: process.cwd(),
  })
  const filesIndexReady: Record<string, PackageFileInfo> = R.fromPairs(
    await Promise.all(
      Object.entries(filesIndex).map(async ([fileName, fileInfo]): Promise<[string, PackageFileInfo]> => {
        const { integrity, checkedAt } = await fileInfo.writeResult
        return [
          fileName,
          {
            ...R.omit(['writeResult'], fileInfo),
            checkedAt,
            integrity: integrity.toString(),
          },
        ]
      })
    )
  )
  await cafs.importPackage(versionDir, {
    filesResponse: {
      filesIndex: filesIndexReady,
      fromStore: false,
    },
    force: true,
  })
}

function getNodeJSTarball (nodeVersion: string) {
  const platform = process.platform === 'win32' ? 'win' : process.platform
  const arch = platform === 'win' && process.arch === 'ia32' ? 'x86' : process.arch
  const extension = platform === 'win' ? 'zip' : 'tar.gz'
  return `https://nodejs.org/download/release/v${nodeVersion}/node-v${nodeVersion}-${platform}-${arch}.${extension}`
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
