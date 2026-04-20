import fs from 'node:fs'
import { mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises'
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

// Range to download when the running Node is too old. Constrained to the
// current major so we don't silently jump majors across releases, and pinned
// above MIN_BUILDER_VERSION.minor so older point releases (e.g. 25.0.x) that
// don't support `--build-sea` aren't picked.
const DEFAULT_BUILDER_SPEC = `>=${MIN_BUILDER_VERSION.major}.${MIN_BUILDER_VERSION.minor}.0 <${MIN_BUILDER_VERSION.major + 1}.0.0`

// Target OS names match `process.platform`. That keeps the CLI surface
// consistent with pnpm's own `--os` flag (which also takes platform constants)
// and with `supportedArchitectures.os` in pnpm-workspace.yaml.
const SUPPORTED_OS = ['linux', 'darwin', 'win32'] as const

const SUPPORTED_TARGETS =
  'linux-x64, linux-x64-musl, linux-arm64, linux-arm64-musl, darwin-x64, darwin-arm64, win32-x64, win32-arm64'

export const commandNames = ['pack-app']

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
      'Pack a CommonJS entry file into a standalone executable for one or more target platforms.\n\n' +
      'The executable embeds a Node.js binary via the Node.js Single Executable Applications API.\n' +
      `Requires Node.js v${MIN_BUILDER_VERSION.major}.${MIN_BUILDER_VERSION.minor}+ to perform ` +
      'the injection. The running Node.js is used when it is new enough; otherwise, the ' +
      `latest Node.js v${MIN_BUILDER_VERSION.major}.${MIN_BUILDER_VERSION.minor}+ in the ` +
      `v${MIN_BUILDER_VERSION.major}.x line is downloaded automatically.`,
    url: docsUrl('pack-app'),
    usages: [
      'pnpm pack-app --entry dist/index.cjs --target linux-x64 --target win32-x64',
      'pnpm pack-app --entry dist/index.cjs --target linux-x64-musl --node-version 22',
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
            description: 'Output directory for the built executables. Defaults to "dist-app".',
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

export type PackAppOptions = Pick<Config,
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

export async function handler (opts: PackAppOptions, params: string[]): Promise<string> {
  const entryPath = opts.entry ?? params[0]
  if (!entryPath) {
    throw new PnpmError('PACK_APP_MISSING_ENTRY',
      '"pnpm pack-app" requires a CJS entry file (pass --entry <path>)')
  }
  const resolvedEntry = path.resolve(opts.dir, entryPath)
  let entryStat: fs.Stats
  try {
    entryStat = fs.statSync(resolvedEntry)
  } catch {
    throw new PnpmError('PACK_APP_ENTRY_NOT_FOUND', `Entry file not found: ${resolvedEntry}`)
  }
  if (!entryStat.isFile()) {
    throw new PnpmError('PACK_APP_ENTRY_NOT_FILE',
      `Entry path must be a regular file: ${resolvedEntry}`)
  }

  const rawTargets = opts.target == null
    ? []
    : Array.isArray(opts.target) ? opts.target : [opts.target]
  if (rawTargets.length === 0) {
    throw new PnpmError('PACK_APP_MISSING_TARGET',
      `"pnpm pack-app" requires at least one --target. Supported: ${SUPPORTED_TARGETS}`)
  }
  const targets = rawTargets.map(parseTarget)

  const outputDir = path.resolve(opts.dir, opts.outputDir ?? 'dist-app')
  await mkdir(outputDir, { recursive: true })

  const outputName = validateOutputName(opts.outputName ?? await readPackageName(opts.dir))
  const requestedNodeSpec = opts.nodeVersion ?? process.version.slice(1)

  const fetch = createFetchFromRegistry(opts)
  const buildRoot = path.join(opts.pnpmHomeDir, 'pack-app')

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
    // Write the SEA config into a fresh, unpredictable temp directory (0700
    // by default) rather than a predictable path under os.tmpdir(). Avoids
    // TOCTOU/symlink attacks on multi-user systems.
    // eslint-disable-next-line no-await-in-loop
    const tmpConfigDir = await mkdtemp(path.join(os.tmpdir(), 'pnpm-pack-app-'))
    const configPath = path.join(tmpConfigDir, 'sea-config.json')
    // eslint-disable-next-line no-await-in-loop
    await writeFile(configPath, JSON.stringify(seaConfig, null, 2), { flag: 'wx' })

    try {
      // eslint-disable-next-line no-await-in-loop
      await execa(builderBin, ['--build-sea', configPath], { stdio: 'inherit' })
    } finally {
      // eslint-disable-next-line no-await-in-loop
      await rm(tmpConfigDir, { recursive: true, force: true }).catch(() => {})
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
  if (opts.libc != null) {
    args.push(`--libc=${opts.libc}`)
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

async function resolveVersion (
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

// Parsed triplet must match this shape exactly. We anchor and constrain each
// segment so that inputs like `linux-x64-musl-../../outside` are rejected
// outright — otherwise `target.raw` would later flow into path.join for the
// output directory and could escape it.
const TARGET_PATTERN = /^(linux|darwin|win32)-(x64|arm64)(?:-(musl))?$/

function parseTarget (raw: string): ParsedTarget {
  const match = TARGET_PATTERN.exec(raw)
  if (!match) {
    throw new PnpmError('PACK_APP_INVALID_TARGET',
      `Invalid target: "${raw}". Expected format: <os>-<arch>[-<libc>] where <os> is ${SUPPORTED_OS.join('|')}, <arch> is x64|arm64, optional <libc> is musl (linux only).`)
  }
  const [, platform, arch, libc] = match
  if (libc === 'musl' && platform !== 'linux') {
    throw new PnpmError('PACK_APP_INVALID_TARGET',
      `The "musl" libc suffix is only valid for linux targets (got "${raw}").`)
  }
  return { raw, platform, arch, libc: libc || undefined }
}

// Reject anything that would let the output escape its target directory when
// joined in `path.join(targetOutputDir, outputName)`.
function validateOutputName (name: string): string {
  if (name !== path.basename(name) || name === '' || name === '.' || name === '..' ||
      name.includes('/') || name.includes('\\') || name.includes('\0')) {
    throw new PnpmError('PACK_APP_INVALID_OUTPUT_NAME',
      `Invalid --output-name "${name}". The name must be a plain filename without path separators.`)
  }
  return name
}

async function readPackageName (dir: string): Promise<string> {
  let raw: string
  try {
    raw = await readFile(path.join(dir, 'package.json'), 'utf8')
  } catch {
    throw new PnpmError('PACK_APP_NO_PACKAGE_NAME',
      `Could not determine --output-name: failed to read package.json in ${dir}`,
      { hint: 'Pass --output-name <name> to set the executable name explicitly.' }
    )
  }
  const manifest = JSON.parse(raw) as { name?: unknown }
  if (typeof manifest.name !== 'string' || manifest.name === '') {
    throw new PnpmError('PACK_APP_NO_PACKAGE_NAME',
      `Could not determine --output-name: package.json in ${dir} has no "name" field`,
      { hint: 'Pass --output-name <name> to set the executable name explicitly.' }
    )
  }
  return manifest.name.replace(/^@[^/]+\//, '')
}

/**
 * SEA injection invalidates the existing code signature on macOS binaries, so
 * the output must be re-signed. Native macOS hosts use `codesign`; Linux hosts
 * cross-signing a darwin target use `ldid`. Windows hosts have no readily
 * available ad-hoc signer, so we refuse to produce an unsigned output silently
 * and tell the user to re-sign on macOS or Linux.
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
      throw new PnpmError('PACK_APP_MACOS_SIGN_FAILED',
        `Cross-compiled macOS binary at ${outputFile} could not be ad-hoc signed with "ldid".`,
        { hint: 'Install ldid (https://github.com/ProcursusTeam/ldid) or re-sign the binary on macOS with "codesign --sign - <file>".' }
      )
    }
    return
  }
  throw new PnpmError('PACK_APP_MACOS_SIGN_UNSUPPORTED_HOST',
    `Cannot ad-hoc sign the macOS binary at ${outputFile} on a ${process.platform} host.`,
    { hint: 'Build macOS targets on a macOS or Linux host, or re-sign the produced binary yourself with "codesign --sign -" on macOS.' }
  )
}
