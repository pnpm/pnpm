import fs from 'node:fs'
import path from 'node:path'

import loadJsonFile from 'load-json-file'
import writeJsonFile from 'write-json-file'

import { globalInfo } from '@pnpm/logger'
import { fetchNode } from '@pnpm/node.fetcher'
import { getStorePath } from '@pnpm/store-path'
import { createFetchFromRegistry } from '@pnpm/fetch'
import type { NvmNodeCommandOptions, FetchFromRegistry } from '@pnpm/types'

import { getNodeMirror } from './getNodeMirror.js'
import { parseNodeSpecifier } from './parseNodeSpecifier.js'

export async function getNodeBinDir(opts: NvmNodeCommandOptions): Promise<string> {
  const fetch = createFetchFromRegistry(opts)

  const nodesDir = getNodeVersionsBaseDir(opts.pnpmHomeDir)

  let wantedNodeVersion =
    opts.useNodeVersion ?? (await readNodeVersionsManifest(nodesDir))?.default

  if (wantedNodeVersion == null) {
    const response = await fetch('https://registry.npmjs.org/node')

    wantedNodeVersion = (await response.json() as any)['dist-tags'].lts // eslint-disable-line

    if (wantedNodeVersion == null) {
      throw new Error('Could not resolve LTS version of Node.js')
    }

    await writeJsonFile(path.join(nodesDir, 'versions.json'), {
      default: wantedNodeVersion,
    })
  }

  const { useNodeVersion, releaseChannel } =
    parseNodeSpecifier(wantedNodeVersion)

  const nodeMirrorBaseUrl = getNodeMirror(opts.rawConfig, releaseChannel)

  const nodeDir = await getNodeDir(fetch, {
    ...opts,
    useNodeVersion,
    nodeMirrorBaseUrl,
  })

  return process.platform === 'win32' ? nodeDir : path.join(nodeDir, 'bin')
}

export function getNodeVersionsBaseDir(pnpmHomeDir: string | undefined): string {
  return path.join(pnpmHomeDir ?? '', 'nodejs')
}

export async function getNodeDir(
  fetch: FetchFromRegistry,
  opts: NvmNodeCommandOptions & {
    useNodeVersion: string
    nodeMirrorBaseUrl: string
  }
): Promise<string> {
  const nodesDir = getNodeVersionsBaseDir(opts.pnpmHomeDir)

  await fs.promises.mkdir(nodesDir, { recursive: true })

  const versionDir = path.join(nodesDir, opts.useNodeVersion)

  if (!fs.existsSync(versionDir)) {
    const storeDir = await getStorePath({
      pkgRoot: process.cwd(),
      storePath: opts.storeDir,
      pnpmHomeDir: opts.pnpmHomeDir,
    })

    const cafsDir = path.join(storeDir, 'files')

    globalInfo(`Fetching Node.js ${opts.useNodeVersion} ...`)

    await fetchNode(fetch, opts.useNodeVersion, versionDir, {
      ...opts,
      cafsDir,
      retry: {
        maxTimeout: opts.fetchRetryMaxtimeout ?? 0,
        minTimeout: opts.fetchRetryMintimeout ?? 0,
        retries: opts.fetchRetries ?? 0,
        factor: opts.fetchRetryFactor ?? 0,
        randomize: false,
      },
    })
  }

  return versionDir
}

async function readNodeVersionsManifest(
  nodesDir: string
): Promise<{ default?: string }> {
  try {
    return await loadJsonFile<{ default?: string }>(
      path.join(nodesDir, 'versions.json')
    )
  } catch (err: any) { // eslint-disable-line
    if (err.code === 'ENOENT') {
      return {}
    }
    throw err
  }
}
