/* eslint-disable no-await-in-loop */
import fs from 'fs'
import os from 'os'
import path from 'path'
import { sync as execaSync } from 'execa'
import { pick } from 'ramda'
import { docsUrl } from '@pnpm/cli-utils'
import { type Config, types as allTypes } from '@pnpm/config'
import { PnpmError } from '@pnpm/error'
import { createFetchFromRegistry } from '@pnpm/fetch'
import { fetchNode } from '@pnpm/node.fetcher'
import { resolveNodeVersion, parseNodeSpecifier, getNodeMirror } from '@pnpm/node.resolver'
import { getStorePath } from '@pnpm/store-path'
import renderHelp from 'render-help'

// Supported target format: <os>-<arch>[-<libc>]
// <os>:   linux | macos | win
// <arch>: x64 | arm64
// <libc>: musl (optional, Linux only)
const TARGET_OS_MAP: Record<string, string> = {
  linux: 'linux',
  macos: 'darwin',
  win: 'win32',
}

export const commandNames = ['build-sea']

export function rcOptionsTypes (): Record<string, unknown> {
  return {}
}

export function cliOptionsTypes (): Record<string, unknown> {
  return {
    ...pick([
      'ca',
      'cert',
      'fetch-retries',
      'fetch-retry-factor',
      'fetch-retry-maxtimeout',
      'fetch-retry-mintimeout',
      'fetch-timeout',
      'https-proxy',
      'key',
      'local-address',
      'no-proxy',
      'store-dir',
      'strict-ssl',
      'user-agent',
    ], allTypes),
    entry: String,
    target: [String, Array],
    'node-version': String,
    'output-dir': String,
    'output-name': String,
  }
}

export const shorthands: Record<string, string> = {
  t: '--target',
  o: '--output-dir',
}

export function help (): string {
  return renderHelp({
    description: 'Build a standalone Single Executable Application (SEA) using Node.js SEA.\n\n'
      + 'Requires Node.js v25.5+ to be available (either the running Node.js or downloaded automatically).',
    url: docsUrl('build-sea'),
    usages: [
      'pnpm build-sea --entry dist/index.cjs --target linux-x64 --target win-x64',
      'pnpm build-sea --entry dist/index.cjs --target linux-x64-musl --node-version 22',
    ],
    descriptionLists: [
      {
        title: 'Options',
        list: [
          {
            description: 'Path to the CJS entry file to embed in the executable',
            name: '--entry',
          },
          {
            description: 'Target to build for. Can be specified multiple times.\n'
              + 'Supported: linux-x64, linux-x64-musl, linux-arm64, linux-arm64-musl, '
              + 'macos-x64, macos-arm64, win-x64, win-arm64',
            name: '--target',
            shortAlias: '-t',
          },
          {
            description: 'Node.js version to embed (e.g. "22", "22.0.0", "lts"). Defaults to the current Node.js version.',
            name: '--node-version',
          },
          {
            description: 'Output directory for the built executables. Defaults to "dist-sea".',
            name: '--output-dir',
            shortAlias: '-o',
          },
          {
            description: 'Name for the output executable without extension. Defaults to the package name.',
            name: '--output-name',
          },
        ],
      },
    ],
  })
}

export type BuildSeaOptions = Pick<Config,
  | 'dir'
  | 'pnpmHomeDir'
  | 'storeDir'
  | 'rawConfig'
  | 'ca'
  | 'cert'
  | 'fetchRetries'
  | 'fetchRetryFactor'
  | 'fetchRetryMaxtimeout'
  | 'fetchRetryMintimeout'
  | 'fetchTimeout'
  | 'httpProxy'
  | 'httpsProxy'
  | 'key'
  | 'localAddress'
  | 'noProxy'
  | 'strictSsl'
  | 'userAgent'
> & Partial<Pick<Config, 'configDir' | 'cliOptions' | 'sslConfigs'>> & {
  entry?: string
  target?: string | string[]
  nodeVersion?: string
  outputDir?: string
  outputName?: string
}

export async function handler (opts: BuildSeaOptions, params: string[]): Promise<string> {
  const entryPath = opts.entry ?? params[0]
  if (!entryPath) {
    throw new PnpmError('MISSING_ENTRY', '"pnpm build-sea" requires an entry file via --entry')
  }

  const resolvedEntry = path.resolve(opts.dir, entryPath)
  if (!fs.existsSync(resolvedEntry)) {
    throw new PnpmError('ENTRY_NOT_FOUND', `Entry file not found: ${resolvedEntry}`)
  }

  const rawTargets = opts.target
  const targets = rawTargets == null
    ? []
    : Array.isArray(rawTargets) ? rawTargets : [rawTargets]

  if (targets.length === 0) {
    throw new PnpmError('MISSING_TARGET',
      '"pnpm build-sea" requires at least one --target.\n'
      + 'Supported: linux-x64, linux-x64-musl, linux-arm64, linux-arm64-musl, '
      + 'macos-x64, macos-arm64, win-x64, win-arm64')
  }

  const outputDir = path.resolve(opts.dir, opts.outputDir ?? 'dist-sea')
  await fs.promises.mkdir(outputDir, { recursive: true })

  const outputName = opts.outputName ?? await readPackageName(opts.dir)
  const nodeVersion = opts.nodeVersion ?? process.version.slice(1)

  const builderPath = await getBuilderPath(opts)

  const results: string[] = []
  for (const target of targets) {
    const parsed = parseTarget(target)

    const download = await resolveAndInstall(opts, nodeVersion, parsed.platform, parsed.arch, parsed.libc)

    if (!download) {
      throw new PnpmError('COULD_NOT_RESOLVE_NODEJS', `Couldn't find Node.js version matching ${nodeVersion}`)
    }

    const { nodeDir, nodeVersion: resolvedVersion } = download
    const nodeBin = parsed.platform === 'win32'
      ? path.join(nodeDir, 'node.exe')
      : path.join(nodeDir, 'bin', 'node')

    const targetOutputDir = path.join(outputDir, target)
    await fs.promises.mkdir(targetOutputDir, { recursive: true })

    const outputFile = parsed.platform === 'win32'
      ? path.join(targetOutputDir, `${outputName}.exe`)
      : path.join(targetOutputDir, outputName)

    const seaConfig = {
      main: resolvedEntry,
      output: outputFile,
      executable: nodeBin,
      disableExperimentalSEAWarning: true,
      useCodeCache: false,
      useSnapshot: false,
    }

    const configPath = path.join(os.tmpdir(), `pnpm-sea-${target}-${Date.now()}.json`)
    await fs.promises.writeFile(configPath, JSON.stringify(seaConfig, null, 2))

    try {
      execaSync(builderPath, ['--build-sea', configPath], { stdio: 'inherit' })
    } finally {
      await fs.promises.unlink(configPath).catch(() => {})
    }

    // Sign macOS binaries — required after SEA injection invalidates the original signature
    if (parsed.platform === 'darwin') {
      if (process.platform === 'darwin') {
        // Ad-hoc sign using codesign (always available on macOS)
        execaSync('codesign', ['--sign', '-', outputFile], { stdio: 'inherit' })
      } else if (process.platform === 'linux') {
        // Ad-hoc sign using ldid when cross-compiling from Linux (must be installed)
        try {
          execaSync('ldid', ['-S', outputFile], { stdio: 'inherit' })
        } catch {
          // ldid not available — skip ad-hoc signing
        }
      }
    }

    results.push(`  ${target}: ${outputFile} (Node.js ${resolvedVersion})`)
  }

  return `Built ${targets.length} executable${targets.length === 1 ? '' : 's'}:\n${results.join('\n')}`
}

// Return a path to a node binary that supports --build-sea (v25.5.0+).
// If the running Node.js already qualifies, use it directly (no download needed).
// Otherwise, download Node.js v25 for the host platform.
async function getBuilderPath (opts: BuildSeaOptions): Promise<string> {
  const [major, minor] = process.version.slice(1).split('.').map(Number)
  if (major > 25 || (major === 25 && minor >= 5)) {
    return process.execPath
  }

  const download = await resolveAndInstall(opts, '25')

  if (!download) {
    throw new PnpmError('COULD_NOT_RESOLVE_NODEJS',
      '"pnpm build-sea" requires Node.js v25.5+ to generate SEA executables.\n'
      + `The current Node.js (${process.version}) is too old and a v25 download could not be resolved.`,
      { hint: 'Upgrade Node.js: pnpm env use --global 25' }
    )
  }

  const { nodeDir } = download
  return process.platform === 'win32'
    ? path.join(nodeDir, 'node.exe')
    : path.join(nodeDir, 'bin', 'node')
}

async function resolveAndInstall (
  opts: BuildSeaOptions,
  envSpecifier: string,
  platform?: string,
  arch?: string,
  libc?: string
): Promise<{ nodeVersion: string, nodeDir: string } | null> {
  const fetch = createFetchFromRegistry(opts)
  const { releaseChannel, versionSpecifier } = parseNodeSpecifier(envSpecifier)
  const nodeMirrorBaseUrl = getNodeMirror(opts.rawConfig, releaseChannel)
  const nodeVersion = await resolveNodeVersion(fetch, versionSpecifier, nodeMirrorBaseUrl)
  if (!nodeVersion) return null

  const nodesDir = path.join(opts.pnpmHomeDir, 'nodejs')
  const targetId = platform && arch
    ? [platform, arch, libc].filter(Boolean).join('-')
    : undefined
  const nodeDir = targetId
    ? path.join(nodesDir, targetId, nodeVersion)
    : path.join(nodesDir, nodeVersion)

  if (!fs.existsSync(nodeDir)) {
    const storeDir = await getStorePath({
      pkgRoot: process.cwd(),
      storePath: opts.storeDir,
      pnpmHomeDir: opts.pnpmHomeDir,
    })
    await fs.promises.mkdir(nodeDir, { recursive: true })
    await fetchNode(fetch, nodeVersion, nodeDir, {
      storeDir,
      platform,
      arch,
      libc,
      fetchTimeout: opts.fetchTimeout,
      retry: {
        maxTimeout: opts.fetchRetryMaxtimeout,
        minTimeout: opts.fetchRetryMintimeout,
        retries: opts.fetchRetries,
        factor: opts.fetchRetryFactor,
      },
    })
  }

  return { nodeVersion, nodeDir }
}

function parseTarget (target: string): { platform: string, arch: string, libc?: string } {
  const [osName, arch, libc] = target.split('-')

  if (!osName || !arch) {
    throw new PnpmError('INVALID_TARGET',
      `Invalid target: "${target}". Expected format: <os>-<arch>[-<libc>] (e.g. linux-x64, linux-x64-musl, macos-arm64, win-x64)`)
  }

  const platform = TARGET_OS_MAP[osName]
  if (!platform) {
    throw new PnpmError('INVALID_TARGET',
      `Unknown OS "${osName}" in target "${target}". Supported: linux, macos, win`)
  }

  if (arch !== 'x64' && arch !== 'arm64') {
    throw new PnpmError('INVALID_TARGET',
      `Unknown arch "${arch}" in target "${target}". Supported: x64, arm64`)
  }

  return { platform, arch, libc }
}

async function readPackageName (dir: string): Promise<string> {
  try {
    const raw = await fs.promises.readFile(path.join(dir, 'package.json'), 'utf8')
    const { name } = JSON.parse(raw) as { name?: unknown }
    if (typeof name === 'string' && name) {
      return name.replace(/^@[^/]+\//, '') // strip scope
    }
  } catch {}
  return 'app'
}
