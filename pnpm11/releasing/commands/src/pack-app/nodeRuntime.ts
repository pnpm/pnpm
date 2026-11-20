import fs from 'node:fs'
import { mkdir, writeFile } from 'node:fs/promises'
import path from 'node:path'

import {
  getNodeMirror,
  parseNodeSpecifier,
  resolveNodeVersion,
} from '@pnpm/engine.runtime.node-resolver'
import { PnpmError } from '@pnpm/error'
import { runPnpmCli } from '@pnpm/exec.pnpm-cli-runner'
import type { createFetchFromRegistry } from '@pnpm/network.fetch'
import { familySync } from 'detect-libc'

/** Minimum Node.js version that supports `node --build-sea`. */
export const MIN_BUILDER_VERSION = { major: 25, minor: 5 } as const

/**
 * Returns a Node.js binary that supports `--build-sea` AND produces a SEA
 * blob the embedded runtime can deserialize. The second constraint forces the
 * builder to match the target runtime version exactly: blobs are versioned by
 * the writer's internal struct layout with no header, and Node bumps that
 * layout in minor releases (e.g. v25.7 added a ModuleFormat byte for ESM
 * entries), so a cross-version blob crashes at startup.
 *
 * Prefers the running interpreter when it already matches the target version;
 * otherwise downloads the target version for the host platform.
 */
export async function resolveBuilderBinary (ctx: {
  buildRoot: string
  targetVersion: string
}): Promise<string> {
  if (runningNodeCanBuildSea() && process.version === `v${ctx.targetVersion}`) {
    return process.execPath
  }
  if (!builderVersionCanBuildSea(ctx.targetVersion)) {
    throw new PnpmError('PACK_APP_RUNTIME_TOO_OLD',
      `The embedded runtime "node@${ctx.targetVersion}" is older than Node.js v${MIN_BUILDER_VERSION.major}.${MIN_BUILDER_VERSION.minor}, which is the minimum version that supports --build-sea.`,
      { hint: `Pass --runtime node@${MIN_BUILDER_VERSION.major}.${MIN_BUILDER_VERSION.minor}.0 (or newer) or set "pnpm.app.runtime" in package.json.` }
    )
  }
  return ensureNodeRuntime({
    buildRoot: ctx.buildRoot,
    version: ctx.targetVersion,
    platform: process.platform,
    arch: process.arch,
    // Pin libc to the host's. Otherwise a caller that had set
    // supportedArchitectures.libc=musl in their config would cause the
    // glibc host to download a musl Node that it cannot execute.
    libc: hostLinuxLibc(),
  })
}

function hostLinuxLibc (): 'glibc' | 'musl' | undefined {
  if (process.platform !== 'linux') return undefined
  const family = familySync()
  return family === 'musl' ? 'musl' : 'glibc'
}

function runningNodeCanBuildSea (): boolean {
  return builderVersionCanBuildSea(process.version.slice(1))
}

function builderVersionCanBuildSea (version: string): boolean {
  const [majorStr, minorStr] = version.split('.')
  const major = Number(majorStr)
  const minor = Number(minorStr)
  return (
    major > MIN_BUILDER_VERSION.major ||
    (major === MIN_BUILDER_VERSION.major && minor >= MIN_BUILDER_VERSION.minor)
  )
}

/**
 * Fetches a Node.js runtime into a dedicated per-target directory under the
 * pnpm home, reusing the cached binary if already present. Actual files are
 * hardlinked from pnpm's content-addressable store, so repeated calls are
 * cheap and `pnpm store prune` can reclaim them.
 */
export async function ensureNodeRuntime (opts: {
  buildRoot: string
  version: string
  platform: string
  arch: string
  libc?: string
}): Promise<string> {
  // Linux variants always need a libc pin (glibc or musl) so that variant
  // selection is deterministic and doesn't depend on the host's detected
  // libc or the user's supportedArchitectures.libc config.
  const libc = opts.platform === 'linux' ? opts.libc ?? 'glibc' : opts.libc
  const targetId = [opts.platform, opts.arch, libc].filter(Boolean).join('-')
  const installDir = path.join(opts.buildRoot, `${targetId}-${opts.version}`)
  const nodeDir = path.join(installDir, 'node_modules', 'node')
  const binaryPath = nodeBinaryPath(nodeDir, opts.platform)
  if (fs.existsSync(binaryPath)) return binaryPath

  await mkdir(installDir, { recursive: true })
  await writeFile(
    path.join(installDir, 'package.json'),
    `${JSON.stringify({ name: `pnpm-pack-app-${targetId}`, private: true }, null, 2)}\n`
  )

  // Flags that select the target variant must come before the positional
  // package spec; otherwise `pnpm add` silently installs the host variant.
  const args = [
    'add',
    '--ignore-scripts',
    '--ignore-workspace',
    `--os=${opts.platform}`,
    `--cpu=${opts.arch}`,
  ]
  if (libc != null) {
    args.push(`--libc=${libc}`)
  }
  args.push(`node@runtime:${opts.version}`)
  runPnpmCli(args, { cwd: installDir })

  if (!fs.existsSync(binaryPath)) {
    throw new PnpmError('PACK_APP_NODE_BINARY_MISSING',
      `Expected Node.js binary at ${binaryPath} after installing node@runtime:${opts.version}, but it was not found.`)
  }
  return binaryPath
}

function nodeBinaryPath (nodeDir: string, platform: string): string {
  return platform === 'win32'
    ? path.join(nodeDir, 'node.exe')
    : path.join(nodeDir, 'bin', 'node')
}

export async function resolveVersion (
  fetch: ReturnType<typeof createFetchFromRegistry>,
  specifier: string,
  nodeDownloadMirrors?: Record<string, string>
): Promise<string> {
  const { releaseChannel, versionSpecifier } = parseNodeSpecifier(specifier)
  const nodeMirrorBaseUrl = getNodeMirror(nodeDownloadMirrors, releaseChannel)
  const version = await resolveNodeVersion(fetch, versionSpecifier, nodeMirrorBaseUrl)
  if (!version) {
    throw new PnpmError('PACK_APP_NODE_VERSION_NOT_FOUND',
      `Could not find a Node.js version that satisfies "${specifier}"`)
  }
  return version
}

// Runtime spec is "<name>@<version>". Only "node" is supported today; the
// prefix is kept so future runtimes (bun, deno) can share the same flag
// without a breaking change. Reading the runtime name rather than a bare
// version also avoids shadowing pnpm's global `node-version` rc setting,
// whose value would otherwise leak into Config['nodeVersion'] and override
// `pnpm.app.runtime`.
const SUPPORTED_RUNTIMES = ['node'] as const
const RUNTIME_PATTERN = /^(node)@(.+)$/

interface ParsedRuntime {
  name: typeof SUPPORTED_RUNTIMES[number]
  version: string
}

export function parseRuntime (spec: string): ParsedRuntime {
  const match = RUNTIME_PATTERN.exec(spec)
  if (!match) {
    throw new PnpmError('PACK_APP_INVALID_RUNTIME',
      `Invalid runtime "${spec}". Expected format: <name>@<version> (supported runtimes: ${SUPPORTED_RUNTIMES.join(', ')}; e.g. "node@${MIN_BUILDER_VERSION.major}.${MIN_BUILDER_VERSION.minor}.0").`)
  }
  return { name: match[1] as ParsedRuntime['name'], version: match[2] }
}
