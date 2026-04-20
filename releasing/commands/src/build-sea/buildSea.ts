import fs from 'node:fs'
import { mkdir, readFile, unlink, writeFile } from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'

import { docsUrl } from '@pnpm/cli.utils'
import type { Config } from '@pnpm/config.reader'
import {
  getNodeMirror,
  parseNodeSpecifier,
  resolveNodeVersion,
} from '@pnpm/engine.runtime.node-resolver'
import { PnpmError } from '@pnpm/error'
import { runPnpmCli } from '@pnpm/exec.pnpm-cli-runner'
import { createFetchFromRegistry } from '@pnpm/network.fetch'
import { safeExeca as execa } from 'execa'
import { renderHelp } from 'render-help'

/** Minimum Node.js version that supports `node --build-sea`. */
const MIN_BUILDER_VERSION = { major: 25, minor: 5 } as const

/** Node.js major to download for building SEAs when the running Node is too old. */
const DEFAULT_BUILDER_SPEC = String(MIN_BUILDER_VERSION.major)

/**
 * Maps target-OS strings accepted on the command line to Node.js's `process.platform`
 * values. We use friendlier names in the CLI ("macos", "win") than the Node.js
 * constants ("darwin", "win32").
 */
const TARGET_OS_MAP: Record<string, string> = {
  linux: 'linux',
  macos: 'darwin',
  win: 'win32',
}

const SUPPORTED_TARGETS =
  'linux-x64, linux-x64-musl, linux-arm64, linux-arm64-musl, macos-x64, macos-arm64, win-x64, win-arm64'

export const commandNames = ['build-sea']

export function rcOptionsTypes (): Record<string, unknown> {
  return {}
}

export function cliOptionsTypes (): Record<string, unknown> {
  return {
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
    description:
      'Build a standalone Single Executable Application (SEA) from a CommonJS entry file.\n\n' +
      `Requires Node.js v${MIN_BUILDER_VERSION.major}.${MIN_BUILDER_VERSION.minor}+ to perform ` +
      'the injection. The running Node.js is used when it is new enough; otherwise, a ' +
      `Node.js v${DEFAULT_BUILDER_SPEC} binary is downloaded automatically.`,
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
            description:
              `Target to build for. May be specified multiple times. Supported: ${SUPPORTED_TARGETS}`,
            name: '--target',
            shortAlias: '-t',
          },
          {
            description:
              'Node.js version to embed in the output executables (e.g. "22", "22.0.0", "lts"). ' +
              'Defaults to the running Node.js version.',
            name: '--node-version',
          },
          {
            description: 'Output directory for the built executables. Defaults to "dist-sea".',
            name: '--output-dir',
            shortAlias: '-o',
          },
          {
            description:
              'Name for the output executable (without extension). Defaults to the unscoped package name.',
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
> & Partial<Pick<Config,
  | 'ca'
  | 'cert'
  | 'configByUri'
  | 'httpProxy'
  | 'httpsProxy'
  | 'key'
  | 'localAddress'
  | 'nodeDownloadMirrors'
  | 'noProxy'
  | 'strictSsl'
  | 'userAgent'
>> & {
  entry?: string
  target?: string | string[]
  nodeVersion?: string
  outputDir?: string
  outputName?: string
}

interface ParsedTarget {
  raw: string
  platform: string
  arch: string
  libc?: string
}

export async function handler (opts: BuildSeaOptions, params: string[]): Promise<string> {
  const entryPath = opts.entry ?? params[0]
  if (!entryPath) {
    throw new PnpmError('BUILD_SEA_MISSING_ENTRY',
      '"pnpm build-sea" requires a CJS entry file (pass --entry <path>)')
  }
  const resolvedEntry = path.resolve(opts.dir, entryPath)
  if (!fs.existsSync(resolvedEntry)) {
    throw new PnpmError('BUILD_SEA_ENTRY_NOT_FOUND', `Entry file not found: ${resolvedEntry}`)
  }

  const rawTargets = opts.target == null
    ? []
    : Array.isArray(opts.target) ? opts.target : [opts.target]
  if (rawTargets.length === 0) {
    throw new PnpmError('BUILD_SEA_MISSING_TARGET',
      `"pnpm build-sea" requires at least one --target. Supported: ${SUPPORTED_TARGETS}`)
  }
  const targets = rawTargets.map(parseTarget)

  const outputDir = path.resolve(opts.dir, opts.outputDir ?? 'dist-sea')
  await mkdir(outputDir, { recursive: true })

  const outputName = opts.outputName ?? await readPackageName(opts.dir)
  const requestedNodeSpec = opts.nodeVersion ?? process.version.slice(1)

  const fetch = createFetchFromRegistry(opts)
  const buildRoot = path.join(opts.pnpmHomeDir, 'build-sea')

  const builderBin = await resolveBuilderBinary({ fetch, nodeDownloadMirrors: opts.nodeDownloadMirrors, buildRoot })
  const resolvedTargetVersion = await resolveVersion(fetch, requestedNodeSpec, opts.nodeDownloadMirrors)

  const results: string[] = []
  for (const target of targets) {
    // eslint-disable-next-line no-await-in-loop
    const embeddedNodeBin = await ensureNodeRuntime({
      buildRoot,
      version: resolvedTargetVersion,
      platform: target.platform,
      arch: target.arch,
      libc: target.libc,
    })

    const targetOutputDir = path.join(outputDir, target.raw)
    // eslint-disable-next-line no-await-in-loop
    await mkdir(targetOutputDir, { recursive: true })

    const outputFile = target.platform === 'win32'
      ? path.join(targetOutputDir, `${outputName}.exe`)
      : path.join(targetOutputDir, outputName)

    const seaConfig = {
      main: resolvedEntry,
      output: outputFile,
      executable: embeddedNodeBin,
      disableExperimentalSEAWarning: true,
      useCodeCache: false,
      useSnapshot: false,
    }
    const configPath = path.join(os.tmpdir(), `pnpm-sea-${target.raw}-${Date.now()}.json`)
    // eslint-disable-next-line no-await-in-loop
    await writeFile(configPath, JSON.stringify(seaConfig, null, 2))

    try {
      // eslint-disable-next-line no-await-in-loop
      await execa(builderBin, ['--build-sea', configPath], { stdio: 'inherit' })
    } finally {
      // eslint-disable-next-line no-await-in-loop
      await unlink(configPath).catch(() => {})
    }

    // eslint-disable-next-line no-await-in-loop
    await adHocSignMacBinary(target, outputFile)

    results.push(`  ${target.raw}: ${outputFile} (Node.js ${resolvedTargetVersion})`)
  }

  return `Built ${targets.length} executable${targets.length === 1 ? '' : 's'}:\n${results.join('\n')}`
}

/**
 * Returns a Node.js binary that supports `--build-sea`. Prefers the running
 * interpreter to avoid a download; falls back to downloading Node.js v25.
 */
async function resolveBuilderBinary (ctx: {
  fetch: ReturnType<typeof createFetchFromRegistry>
  nodeDownloadMirrors?: Record<string, string>
  buildRoot: string
}): Promise<string> {
  if (runningNodeCanBuildSea()) {
    return process.execPath
  }
  const version = await resolveVersion(ctx.fetch, DEFAULT_BUILDER_SPEC, ctx.nodeDownloadMirrors)
  return ensureNodeRuntime({
    buildRoot: ctx.buildRoot,
    version,
    platform: process.platform,
    arch: process.arch,
  })
}

function runningNodeCanBuildSea (): boolean {
  const [majorStr, minorStr] = process.version.slice(1).split('.')
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
async function ensureNodeRuntime (opts: {
  buildRoot: string
  version: string
  platform: string
  arch: string
  libc?: string
}): Promise<string> {
  const targetId = [opts.platform, opts.arch, opts.libc].filter(Boolean).join('-')
  const installDir = path.join(opts.buildRoot, `${targetId}-${opts.version}`)
  const nodeDir = path.join(installDir, 'node_modules', 'node')
  const binaryPath = nodeBinaryPath(nodeDir, opts.platform)
  if (fs.existsSync(binaryPath)) return binaryPath

  await mkdir(installDir, { recursive: true })
  await writeFile(
    path.join(installDir, 'package.json'),
    `${JSON.stringify({ name: `pnpm-build-sea-${targetId}`, private: true }, null, 2)}\n`
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
  if (opts.libc != null) {
    args.push(`--libc=${opts.libc}`)
  }
  args.push(`node@runtime:${opts.version}`)
  runPnpmCli(args, { cwd: installDir })

  if (!fs.existsSync(binaryPath)) {
    throw new PnpmError('BUILD_SEA_NODE_BINARY_MISSING',
      `Expected Node.js binary at ${binaryPath} after installing node@runtime:${opts.version}, but it was not found.`)
  }
  return binaryPath
}

function nodeBinaryPath (nodeDir: string, platform: string): string {
  return platform === 'win32'
    ? path.join(nodeDir, 'node.exe')
    : path.join(nodeDir, 'bin', 'node')
}

async function resolveVersion (
  fetch: ReturnType<typeof createFetchFromRegistry>,
  specifier: string,
  nodeDownloadMirrors?: Record<string, string>
): Promise<string> {
  const { releaseChannel, versionSpecifier } = parseNodeSpecifier(specifier)
  const nodeMirrorBaseUrl = getNodeMirror(nodeDownloadMirrors, releaseChannel)
  const version = await resolveNodeVersion(fetch, versionSpecifier, nodeMirrorBaseUrl)
  if (!version) {
    throw new PnpmError('BUILD_SEA_NODE_VERSION_NOT_FOUND',
      `Could not find a Node.js version that satisfies "${specifier}"`)
  }
  return version
}

function parseTarget (raw: string): ParsedTarget {
  const [osName, arch, libc] = raw.split('-')
  if (!osName || !arch) {
    throw new PnpmError('BUILD_SEA_INVALID_TARGET',
      `Invalid target: "${raw}". Expected format: <os>-<arch>[-<libc>] (e.g. linux-x64, linux-x64-musl, macos-arm64, win-x64)`)
  }
  const platform = TARGET_OS_MAP[osName]
  if (!platform) {
    throw new PnpmError('BUILD_SEA_INVALID_TARGET',
      `Unknown OS "${osName}" in target "${raw}". Supported: linux, macos, win`)
  }
  if (arch !== 'x64' && arch !== 'arm64') {
    throw new PnpmError('BUILD_SEA_INVALID_TARGET',
      `Unknown arch "${arch}" in target "${raw}". Supported: x64, arm64`)
  }
  if (libc != null && libc !== 'musl') {
    throw new PnpmError('BUILD_SEA_INVALID_TARGET',
      `Unknown libc "${libc}" in target "${raw}". Only "musl" is supported.`)
  }
  if (libc === 'musl' && platform !== 'linux') {
    throw new PnpmError('BUILD_SEA_INVALID_TARGET',
      `The "musl" libc suffix is only valid for linux targets (got "${raw}").`)
  }
  return { raw, platform, arch, libc }
}

async function readPackageName (dir: string): Promise<string> {
  let raw: string
  try {
    raw = await readFile(path.join(dir, 'package.json'), 'utf8')
  } catch {
    throw new PnpmError('BUILD_SEA_NO_PACKAGE_NAME',
      `Could not determine --output-name: failed to read package.json in ${dir}`,
      { hint: 'Pass --output-name <name> to set the executable name explicitly.' }
    )
  }
  const manifest = JSON.parse(raw) as { name?: unknown }
  if (typeof manifest.name !== 'string' || manifest.name === '') {
    throw new PnpmError('BUILD_SEA_NO_PACKAGE_NAME',
      `Could not determine --output-name: package.json in ${dir} has no "name" field`,
      { hint: 'Pass --output-name <name> to set the executable name explicitly.' }
    )
  }
  return manifest.name.replace(/^@[^/]+\//, '')
}

/**
 * SEA injection invalidates the existing code signature on macOS binaries, so
 * the output must be re-signed. For native macOS builds we use `codesign` (ships
 * with Xcode command line tools). When cross-compiling on Linux we use `ldid`
 * when it is available; otherwise the binary is left unsigned and users will
 * need to sign it themselves before distribution.
 */
async function adHocSignMacBinary (target: ParsedTarget, outputFile: string): Promise<void> {
  if (target.platform !== 'darwin') return
  if (process.platform === 'darwin') {
    await execa('codesign', ['--sign', '-', outputFile], { stdio: 'inherit' })
    return
  }
  if (process.platform === 'linux') {
    try {
      await execa('ldid', ['-S', outputFile], { stdio: 'inherit' })
    } catch {
      throw new PnpmError('BUILD_SEA_MACOS_SIGN_FAILED',
        `Cross-compiled macOS binary at ${outputFile} could not be ad-hoc signed with "ldid".`,
        { hint: 'Install ldid (https://github.com/ProcursusTeam/ldid) or re-sign the binary on macOS with "codesign --sign - <file>".' }
      )
    }
  }
}
