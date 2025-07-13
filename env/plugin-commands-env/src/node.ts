import fs from 'fs'
import path from 'path'
import util from 'util'
import { type Config } from '@pnpm/config'
import { getSystemNodeVersion } from '@pnpm/env.system-node-version'
import { createFetchFromRegistry, type FetchFromRegistry } from '@pnpm/fetch'
import { globalInfo, globalWarn } from '@pnpm/logger'
import { fetchNode } from '@pnpm/node.fetcher'
import { getStorePath } from '@pnpm/store-path'
import { type PrepareExecutionEnvOptions, type PrepareExecutionEnvResult } from '@pnpm/types'
import loadJsonFile from 'load-json-file'
import writeJsonFile from 'write-json-file'
import { getNodeMirror } from './getNodeMirror'
import { isValidVersion, parseNodeSpecifier } from './parseNodeSpecifier'
import {version} from 'os'

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

const nodeFetchPromises: Record<string, Promise<GetNodeBinDir>> = {}

export async function prepareExecutionEnv (
  config: NvmNodeCommandOptions,
  { extraBinPaths, executionEnv }: PrepareExecutionEnvOptions
): Promise<PrepareExecutionEnvResult> {
  if (!executionEnv?.nodeVersion || `v${executionEnv.nodeVersion}` === getSystemNodeVersion()) {
    return { extraBinPaths: extraBinPaths ?? [] }
  }

  const { dir } = await resolveRuntime(config, executionEnv.nodeVersion)
  return {
    extraBinPaths: [dir, ...extraBinPaths ?? []],
  }
}

export interface GetNodeBinDir {
  dir: string
  integrity: string
}

export async function resolveRuntime (
  config: NvmNodeCommandOptions,
  nodeVersion: string,
  opts?: {
    expectedVersionIntegrity?: string
  }
) {
  let nodePathPromise = nodeFetchPromises[nodeVersion]
  if (!nodePathPromise) {
    nodePathPromise = getNodeBinDir({
      ...config,
      useNodeVersion: nodeVersion,
    })
    nodeFetchPromises[nodeVersion] = nodePathPromise
  }

  return nodePathPromise
}

export async function getNodeBinDir (opts: NvmNodeCommandOptions): Promise<GetNodeBinDir> {
  const fetch = createFetchFromRegistry(opts)
  const nodesDir = getNodeVersionsBaseDir(opts.pnpmHomeDir)
  const manifestNodeVersion = (await readNodeVersionsManifest(nodesDir))?.default
  let wantedNodeVersion = opts.useNodeVersion ?? manifestNodeVersion
  if (opts.useNodeVersion != null) {
    // If the user has specified an invalid version via use-node-version, we should not throw an error. Or else, it will break all the commands.
    // Instead, we should fallback to the manifest node version
    if (!isValidVersion(opts.useNodeVersion)) {
      globalWarn(`"${opts.useNodeVersion}" is not a valid Node.js version.`)
      wantedNodeVersion = manifestNodeVersion
    }
  }
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
  const { versionDir: nodeDir, versionIntegrity } = await getNodeDir(fetch, {
    ...opts,
    useNodeVersion,
    nodeMirrorBaseUrl,
  })
  return {
    dir: process.platform === 'win32' ? nodeDir : path.join(nodeDir, 'bin'),
    integrity: versionIntegrity,
  }
}

export function getNodeVersionsBaseDir (pnpmHomeDir: string): string {
  return path.join(pnpmHomeDir, 'nodejs')
}

export async function getNodeDir (
  fetch: FetchFromRegistry,
  opts: NvmNodeCommandOptions & { useNodeVersion: string, nodeMirrorBaseUrl: string }
): Promise<{ versionIntegrity: string, versionDir: string }> {
  const nodesDir = getNodeVersionsBaseDir(opts.pnpmHomeDir)
  await fs.promises.mkdir(nodesDir, { recursive: true })
  const versionDir = path.join(nodesDir, opts.useNodeVersion)
  let versionIntegrity = await readIntegrityFile(versionDir)
  if (versionIntegrity == null) {
    const storeDir = await getStorePath({
      pkgRoot: process.cwd(),
      storePath: opts.storeDir,
      pnpmHomeDir: opts.pnpmHomeDir,
    })
    globalInfo(`Fetching Node.js ${opts.useNodeVersion} ...`)
    versionIntegrity = await fetchNode(fetch, opts.useNodeVersion, versionDir, {
      ...opts,
      storeDir,
      retry: {
        maxTimeout: opts.fetchRetryMaxtimeout,
        minTimeout: opts.fetchRetryMintimeout,
        retries: opts.fetchRetries,
        factor: opts.fetchRetryFactor,
      },
    })
  }
  return { versionIntegrity, versionDir }
}

async function readIntegrityFile (versionDir: string): Promise<string | null> {
  try {
    return fs.promises.readFile(path.join(versionDir, 'integrity'), 'utf8')
  } catch {
    return null
  }
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
