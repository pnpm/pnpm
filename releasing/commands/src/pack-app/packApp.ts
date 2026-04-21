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
import { familySync } from 'detect-libc'
import { safeExeca as execa } from 'execa'
import { renderHelp } from 'render-help'

/** Minimum Node.js version that supports `node --build-sea`. */
const MIN_BUILDER_VERSION = { major: 25, minor: 5 } as const

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
    runtime: String,
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
      'the injection. SEA blobs are not compatible across Node.js minor releases, so the ' +
      'builder Node.js must match the embedded runtime version exactly. The running Node.js ' +
      'is used when it already matches; otherwise a host-arch Node.js of the embedded runtime ' +
      'version is downloaded automatically.\n\n' +
      'Defaults for --entry, --target, --runtime, --output-dir, and --output-name can be ' +
      'set in the package.json under "pnpm.app". CLI flags override the config; --target entirely ' +
      'replaces the configured list so you can narrow it at invocation time.',
    url: docsUrl('pack-app'),
    usages: [
      'pnpm pack-app --entry dist/index.cjs --target linux-x64 --target win32-x64',
      `pnpm pack-app --entry dist/index.cjs --target linux-x64-musl --runtime node@${MIN_BUILDER_VERSION.major}`,
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
              'Runtime to embed in the output executables, as a "<name>@<version>" spec ' +
              `(e.g. "node@${MIN_BUILDER_VERSION.major}", "node@${MIN_BUILDER_VERSION.major}.${MIN_BUILDER_VERSION.minor}.0"). ` +
              `Only "node" is supported today, and the version must be >= v${MIN_BUILDER_VERSION.major}.${MIN_BUILDER_VERSION.minor} (the minimum that supports --build-sea). ` +
              'Defaults to the running Node.js version.',
            name: '--runtime',
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
  runtime?: string
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
  // pnpm.app in package.json supplies defaults for every flag. CLI flags win,
  // but `--target` entirely replaces the config list (additive merging would
  // prevent narrowing from the CLI). See ProjectAppConfig below for the shape.
  const project = await readProjectAppConfig(opts.dir)

  const entryPath = opts.entry ?? params[0] ?? project.app?.entry
  if (!entryPath) {
    throw new PnpmError('PACK_APP_MISSING_ENTRY',
      '"pnpm pack-app" requires a CJS entry file — pass --entry <path> or set "pnpm.app.entry" in package.json.')
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

  const cliTargets = opts.target == null
    ? undefined
    : Array.isArray(opts.target) ? opts.target : [opts.target]
  const rawTargets = cliTargets ?? project.app?.targets ?? []
  if (rawTargets.length === 0) {
    throw new PnpmError('PACK_APP_MISSING_TARGET',
      `"pnpm pack-app" requires at least one target — pass --target <triplet> or set "pnpm.app.targets" in package.json. Supported: ${SUPPORTED_TARGETS}`)
  }
  const targets = rawTargets.map(parseTarget)

  // Parse the runtime before output-name derivation and any network work so
  // that a malformed --runtime fails fast with a clear error instead of being
  // masked by later problems (missing package.json name, registry lookup, etc.).
  const runtimeSpec = opts.runtime ?? project.app?.runtime ?? `node@${process.version.slice(1)}`
  const { version: requestedNodeSpec } = parseRuntime(runtimeSpec)

  const outputDir = path.resolve(opts.dir, opts.outputDir ?? project.app?.outputDir ?? 'dist-app')
  await mkdir(outputDir, { recursive: true })

  const outputName = validateOutputName(opts.outputName ?? project.app?.outputName ?? deriveOutputNameFromPackage(project, opts.dir))

  const fetch = createFetchFromRegistry(opts)
  const buildRoot = path.join(opts.pnpmHomeDir, 'pack-app')

  // Resolve the embedded target version first so the builder can be pinned to
  // the same version. SEA blobs carry no version header and the serialized
  // format has changed across Node.js minor releases (e.g. v25.7 added a
  // ModuleFormat byte for ESM entry points), so a blob produced by a builder
  // of a different version than the embedded runtime will fail deserialization
  // at startup with an opaque native assertion.
  const resolvedTargetVersion = await resolveVersion(fetch, requestedNodeSpec, opts.nodeDownloadMirrors)
  const builderBin = await resolveBuilderBinary({
    buildRoot,
    targetVersion: resolvedTargetVersion,
  })

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
async function resolveBuilderBinary (ctx: {
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
async function ensureNodeRuntime (opts: {
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

function parseRuntime (spec: string): ParsedRuntime {
  const match = RUNTIME_PATTERN.exec(spec)
  if (!match) {
    throw new PnpmError('PACK_APP_INVALID_RUNTIME',
      `Invalid runtime "${spec}". Expected format: <name>@<version> (supported runtimes: ${SUPPORTED_RUNTIMES.join(', ')}; e.g. "node@${MIN_BUILDER_VERSION.major}.${MIN_BUILDER_VERSION.minor}.0").`)
  }
  return { name: match[1] as ParsedRuntime['name'], version: match[2] }
}

// Characters that Win32 rejects in filenames, plus NUL. Path separators are
// checked separately via `path.basename` so the message is crisp.
const INVALID_FILENAME_CHARS = /[<>:"|?*\0]/
// Win32 reserved device names (case-insensitive, with or without an extension).
const RESERVED_WINDOWS_NAME = /^(?:con|prn|aux|nul|com[1-9]|lpt[1-9])(?:\..*)?$/i

// Reject anything that would let the output escape its target directory, or
// that would fail filesystem-level validation on any supported host. This
// surfaces problems at `pack-app` invocation time instead of letting them
// blow up later in `writeFile(outputFile, …)`.
function validateOutputName (name: string): string {
  if (
    name !== path.basename(name) ||
    name === '' || name === '.' || name === '..' ||
    name.includes('/') || name.includes('\\') ||
    INVALID_FILENAME_CHARS.test(name) ||
    RESERVED_WINDOWS_NAME.test(name) ||
    /[. ]$/.test(name)
  ) {
    throw new PnpmError('PACK_APP_INVALID_OUTPUT_NAME',
      `Invalid --output-name "${name}". The name must be a plain filename without path separators, Windows-reserved names (e.g. CON, NUL), characters like <>:"|?* or NUL, and must not end in a dot or space.`)
  }
  return name
}

/** Fields pack-app reads from `pnpm.app` in package.json. */
export interface ProjectAppConfig {
  entry?: string
  targets?: string[]
  runtime?: string
  outputDir?: string
  outputName?: string
}

interface ReadProjectAppConfigResult {
  name?: string
  app?: ProjectAppConfig
}

// A narrow reader just for this command. Using readProjectManifest from
// @pnpm/cli.utils would pull in the installable/engine checks, which are
// irrelevant here: pack-app doesn't need the current project to be installable
// under the running Node, just to have a package.json with optional settings.
async function readProjectAppConfig (dir: string): Promise<ReadProjectAppConfigResult> {
  let raw: string
  try {
    raw = await readFile(path.join(dir, 'package.json'), 'utf8')
  } catch {
    return {}
  }
  let manifest: unknown
  try {
    manifest = JSON.parse(raw)
  } catch (err) {
    throw new PnpmError('PACK_APP_INVALID_PACKAGE_JSON',
      `Failed to parse ${path.join(dir, 'package.json')}: ${(err as Error).message}`)
  }
  if (!isObject(manifest)) return {}

  const name = typeof manifest.name === 'string' && manifest.name !== '' ? manifest.name : undefined
  const pnpmField = isObject(manifest.pnpm) ? manifest.pnpm : undefined
  const appField = pnpmField && isObject(pnpmField.app) ? pnpmField.app : undefined
  if (!appField) return { name }
  return { name, app: validateAppConfig(appField) }
}

function validateAppConfig (raw: Record<string, unknown>): ProjectAppConfig {
  const known = new Set(['entry', 'targets', 'runtime', 'outputDir', 'outputName'])
  for (const key of Object.keys(raw)) {
    if (!known.has(key)) {
      throw new PnpmError('PACK_APP_INVALID_CONFIG',
        `Unknown "pnpm.app.${key}" setting in package.json. Allowed keys: ${Array.from(known).join(', ')}.`)
    }
  }
  const config: ProjectAppConfig = {}
  if (raw.entry != null) {
    if (typeof raw.entry !== 'string') {
      throw new PnpmError('PACK_APP_INVALID_CONFIG', '"pnpm.app.entry" must be a string.')
    }
    config.entry = raw.entry
  }
  if (raw.targets != null) {
    if (!Array.isArray(raw.targets) || !raw.targets.every((t): t is string => typeof t === 'string')) {
      throw new PnpmError('PACK_APP_INVALID_CONFIG', '"pnpm.app.targets" must be an array of strings.')
    }
    config.targets = raw.targets
  }
  if (raw.runtime != null) {
    if (typeof raw.runtime !== 'string') {
      throw new PnpmError('PACK_APP_INVALID_CONFIG', '"pnpm.app.runtime" must be a string.')
    }
    config.runtime = raw.runtime
  }
  if (raw.outputDir != null) {
    if (typeof raw.outputDir !== 'string') {
      throw new PnpmError('PACK_APP_INVALID_CONFIG', '"pnpm.app.outputDir" must be a string.')
    }
    config.outputDir = raw.outputDir
  }
  if (raw.outputName != null) {
    if (typeof raw.outputName !== 'string') {
      throw new PnpmError('PACK_APP_INVALID_CONFIG', '"pnpm.app.outputName" must be a string.')
    }
    config.outputName = raw.outputName
  }
  return config
}

function deriveOutputNameFromPackage (project: ReadProjectAppConfigResult, dir: string): string {
  if (!project.name) {
    throw new PnpmError('PACK_APP_NO_OUTPUT_NAME',
      `Could not determine the output name: package.json in ${dir} has no "name" field.`,
      { hint: 'Pass --output-name <name> or set "pnpm.app.outputName" in package.json.' }
    )
  }
  // Strip @scope/ prefix from scoped packages so the binary name is a plain
  // filename instead of "scope/name". The second validateOutputName() pass
  // downstream rejects any leftover path separators.
  return project.name.replace(/^@[^/]+\//, '')
}

function isObject (value: unknown): value is Record<string, unknown> {
  return value != null && typeof value === 'object' && !Array.isArray(value)
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
