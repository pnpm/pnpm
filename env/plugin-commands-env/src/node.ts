import fs from 'fs'
import path from 'path'
import util from 'util'
import { type Config } from '@pnpm/config'
import { createFetchFromRegistry, type FetchFromRegistry } from '@pnpm/fetch'
import { globalInfo } from '@pnpm/logger'
import { fetchNode } from '@pnpm/node.fetcher'
import { getStorePath } from '@pnpm/store-path'
import loadJsonFile from 'load-json-file'
import writeJsonFile from 'write-json-file'
import { getNodeMirror } from './getNodeMirror'
import { parseNodeSpecifier } from './parseNodeSpecifier'
import { replaceOrAddNodeIntoBinPaths } from './replaceOrAddNodeIntoBinPaths'

export type NvmNodeCommandOptions = Pick<Config,
| 'bin'
| 'global'
| 'fetchRetries'
| 'fetchRetryFactor'
| 'fetchRetryMaxtimeout'
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
| 'rawConfig'
| 'strictSsl'
| 'storeDir'
| 'useNodeVersion'
| 'pnpmHomeDir'
> & Partial<Pick<Config, 'configDir' | 'cliOptions' | 'sslConfigs'>> & {
  remote?: boolean
}

export type ConfigWithExtraBinPaths = NvmNodeCommandOptions & Partial<Pick<Config, 'extraBinPaths'>>

export async function createBinPathsWithNodeVersion (config: ConfigWithExtraBinPaths, nodeVersion: string): Promise<string[]> {
  const baseDir = getNodeVersionsBaseDir(config.pnpmHomeDir)

  const nodePath = await getNodeBinDir({
    ...config,
    useNodeVersion: nodeVersion,
  })

  return replaceOrAddNodeIntoBinPaths(config.extraBinPaths ?? [], baseDir, nodePath)
}

export interface ManifestWithUseNodeVersion {
  pnpm?: {
    useNodeVersion?: string
  }
}

export type PrepareExecutionEnv = (extraBinPaths: string[], useNodeVersion?: string) => Promise<string[]>

export const createPrepareExecutionEnv = (
  config: NvmNodeCommandOptions,
  cache: Partial<Record<string, Promise<string>>> = {}
): PrepareExecutionEnv => async (binPaths, useNodeVersion) => {
  if (!useNodeVersion) return binPaths

  const baseDir = getNodeVersionsBaseDir(config.pnpmHomeDir)

  let nodePathPromise = cache[useNodeVersion]
  if (!nodePathPromise) {
    nodePathPromise = getNodeBinDir({
      ...config,
      useNodeVersion,
    })
    cache[useNodeVersion] = nodePathPromise
  }

  return replaceOrAddNodeIntoBinPaths(binPaths, baseDir, await nodePathPromise)
}

export async function getNodeBinDir (opts: NvmNodeCommandOptions): Promise<string> {
  const fetch = createFetchFromRegistry(opts)
  const nodesDir = getNodeVersionsBaseDir(opts.pnpmHomeDir)
  let wantedNodeVersion = opts.useNodeVersion ?? (await readNodeVersionsManifest(nodesDir))?.default
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
  const { useNodeVersion, releaseChannel } = parseNodeSpecifier(wantedNodeVersion)
  const nodeMirrorBaseUrl = getNodeMirror(opts.rawConfig, releaseChannel)
  const nodeDir = await getNodeDir(fetch, {
    ...opts,
    useNodeVersion,
    nodeMirrorBaseUrl,
  })
  return process.platform === 'win32' ? nodeDir : path.join(nodeDir, 'bin')
}

export function getNodeVersionsBaseDir (pnpmHomeDir: string): string {
  return path.join(pnpmHomeDir, 'nodejs')
}

export async function getNodeDir (fetch: FetchFromRegistry, opts: NvmNodeCommandOptions & { useNodeVersion: string, nodeMirrorBaseUrl: string }): Promise<string> {
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
        maxTimeout: opts.fetchRetryMaxtimeout,
        minTimeout: opts.fetchRetryMintimeout,
        retries: opts.fetchRetries,
        factor: opts.fetchRetryFactor,
      },
    })
  }
  return versionDir
}

async function readNodeVersionsManifest (nodesDir: string): Promise<{ default?: string }> {
  try {
    return await loadJsonFile<{ default?: string }>(path.join(nodesDir, 'versions.json'))
  } catch (err: unknown) {
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') {
      return {}
    }
    throw err
  }
}
