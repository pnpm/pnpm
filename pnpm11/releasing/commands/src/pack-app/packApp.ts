import fs from 'node:fs'
import { mkdir, mkdtemp, rm, writeFile } from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'

import { docsUrl } from '@pnpm/cli.utils'
import type { Config } from '@pnpm/config.reader'
import { PnpmError } from '@pnpm/error'
import { createFetchFromRegistry } from '@pnpm/network.fetch'
import { safeExeca as execa } from 'execa'
import { renderHelp } from 'render-help'

import { deriveOutputNameFromPackage, readProjectAppConfig, type ReadProjectAppConfigResult } from './appConfig.js'
import { ensureNodeRuntime, MIN_BUILDER_VERSION, parseRuntime, resolveBuilderBinary, resolveVersion } from './nodeRuntime.js'
import {
  entryOutsideProjectError,
  isWithinDir,
  outputDirOutsideProjectError,
  outputFileName,
  pathEscapesProject,
  rejectNonRegularOutputFile,
  validateOutputName,
} from './outputPaths.js'
import { adHocSignMacBinary } from './signMacBinary.js'
import { type ParsedTarget, parseTarget, SUPPORTED_TARGETS } from './targets.js'

export type { ProjectAppConfig } from './appConfig.js'

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
    description: helpDescription(),
    url: docsUrl('pack-app'),
    usages: [
      'pnpm pack-app --entry dist/index.cjs --target linux-x64 --target win32-x64',
      `pnpm pack-app --entry dist/index.cjs --target linux-x64-musl --runtime node@${MIN_BUILDER_VERSION.major}`,
    ],
    descriptionLists: [
      {
        title: 'Options',
        list: helpOptions(),
      },
    ],
  })
}

function helpDescription (): string {
  return 'Pack a CommonJS entry file into a standalone executable for one or more target platforms.\n\n' +
    'The executable embeds a Node.js binary via the Node.js Single Executable Applications API.\n' +
    `Requires Node.js v${MIN_BUILDER_VERSION.major}.${MIN_BUILDER_VERSION.minor}+ to perform ` +
    'the injection. SEA blobs are not compatible across Node.js minor releases, so the ' +
    'builder Node.js must match the embedded runtime version exactly. The running Node.js ' +
    'is used when it already matches; otherwise a host-arch Node.js of the embedded runtime ' +
    'version is downloaded automatically.\n\n' +
    'Defaults for --entry, --target, --runtime, --output-dir, and --output-name can be ' +
    'set in the package.json under "pnpm.app". CLI flags override the config; --target entirely ' +
    'replaces the configured list so you can narrow it at invocation time.'
}

function helpOptions (): Array<{ description: string, name: string, shortAlias?: string }> {
  return [
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
  ]
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

export async function handler (opts: PackAppOptions, params: string[]): Promise<string> {
  const plan = await planPackApp(opts, params)
  return buildExecutables(opts, plan)
}

interface PackAppPlan {
  resolvedEntry: string
  targets: ParsedTarget[]
  requestedNodeSpec: string
  outputDir: string
  outputName: string
}

async function planPackApp (opts: PackAppOptions, params: string[]): Promise<PackAppPlan> {
  // pnpm.app in package.json supplies defaults for every flag. CLI flags win,
  // but `--target` entirely replaces the config list (additive merging would
  // prevent narrowing from the CLI). See ProjectAppConfig for the shape.
  const project = await readProjectAppConfig(opts.dir)

  const resolvedEntry = resolveEntry(opts, params, project)
  const targets = resolveTargets(opts.target, project)

  // Parse the runtime before output-name derivation and any network work so
  // that a malformed --runtime fails fast with a clear error instead of being
  // masked by later problems (missing package.json name, registry lookup, etc.).
  const runtimeSpec = opts.runtime ?? project.app?.runtime ?? `node@${process.version.slice(1)}`
  const { version: requestedNodeSpec } = parseRuntime(runtimeSpec)

  const outputDir = await prepareOutputDir(opts, project)

  const outputName = validateOutputName(opts.outputName ?? project.app?.outputName ?? deriveOutputNameFromPackage(project, opts.dir))

  // Reject a pre-existing symlink (or any non-regular file) at any target's
  // final output path before downloading anything: a repo could commit
  // `dist-app/<target>/<name>` as a symlink pointing outside the project, and
  // `node --build-sea` would follow it to overwrite an arbitrary file. The
  // directory containment checks do not cover the leaf file.
  for (const target of targets) {
    rejectNonRegularOutputFile(path.join(outputDir, target.raw, outputFileName(outputName, target.platform)))
  }
  return { resolvedEntry, targets, requestedNodeSpec, outputDir, outputName }
}

function resolveEntry (opts: PackAppOptions, params: string[], project: ReadProjectAppConfigResult): string {
  const entryPath = opts.entry ?? params[0] ?? project.app?.entry
  if (!entryPath) {
    throw new PnpmError('PACK_APP_MISSING_ENTRY',
      '"pnpm pack-app" requires a CJS entry file — pass --entry <path> or set "pnpm.app.entry" in package.json.')
  }
  // `entry` may come from a repo-controlled package.json, so reject absolute
  // paths and `..` traversal before touching the filesystem: the entry's
  // contents get embedded into the produced executable, so an escaping path
  // could exfiltrate a host file (e.g. an SSH key) into a distributable binary.
  if (pathEscapesProject(entryPath)) {
    throw entryOutsideProjectError(entryPath)
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
  // Defense in depth against a same-name symlink that points out of the
  // project: resolve symlinks and require the real path to stay within the
  // (also symlink-resolved) project directory.
  if (!isWithinDir(resolvedEntry, opts.dir)) {
    throw entryOutsideProjectError(entryPath)
  }
  return resolvedEntry
}

function resolveTargets (cliTarget: string | string[] | undefined, project: ReadProjectAppConfigResult): ParsedTarget[] {
  const cliTargets = cliTarget == null
    ? undefined
    : Array.isArray(cliTarget) ? cliTarget : [cliTarget]
  const rawTargets = cliTargets ?? project.app?.targets ?? []
  if (rawTargets.length === 0) {
    throw new PnpmError('PACK_APP_MISSING_TARGET',
      `"pnpm pack-app" requires at least one target — pass --target <triplet> or set "pnpm.app.targets" in package.json. Supported: ${SUPPORTED_TARGETS}`)
  }
  return rawTargets.map(parseTarget)
}

async function prepareOutputDir (opts: PackAppOptions, project: ReadProjectAppConfigResult): Promise<string> {
  // `outputDir` is likewise repo-controllable; reject absolute paths and `..`
  // traversal so build artifacts cannot be written outside the project directory.
  const outputDirRaw = opts.outputDir ?? project.app?.outputDir ?? 'dist-app'
  if (pathEscapesProject(outputDirRaw)) {
    throw outputDirOutsideProjectError(outputDirRaw)
  }
  const outputDir = path.resolve(opts.dir, outputDirRaw)
  await mkdir(outputDir, { recursive: true })
  // Defense in depth against a symlinked output directory that points out of
  // the project: the lexical check above can't see through a symlink, so
  // re-check containment once the real path exists.
  if (!isWithinDir(outputDir, opts.dir)) {
    throw outputDirOutsideProjectError(outputDirRaw)
  }
  return outputDir
}

interface TargetBuildContext {
  plan: PackAppPlan
  dir: string
  buildRoot: string
  builderBin: string
  nodeVersion: string
}

async function buildExecutables (opts: PackAppOptions, plan: PackAppPlan): Promise<string> {
  const fetch = createFetchFromRegistry(opts)
  const buildRoot = path.join(opts.pnpmHomeDir, 'pack-app')

  // Resolve the embedded target version first so the builder can be pinned to
  // the same version. SEA blobs carry no version header and the serialized
  // format has changed across Node.js minor releases (e.g. v25.7 added a
  // ModuleFormat byte for ESM entry points), so a blob produced by a builder
  // of a different version than the embedded runtime will fail deserialization
  // at startup with an opaque native assertion.
  const nodeVersion = await resolveVersion(fetch, plan.requestedNodeSpec, opts.nodeDownloadMirrors)
  const builderBin = await resolveBuilderBinary({
    buildRoot,
    targetVersion: nodeVersion,
  })

  const ctx: TargetBuildContext = { plan, dir: opts.dir, buildRoot, builderBin, nodeVersion }
  const results: string[] = []
  /* eslint-disable no-await-in-loop -- targets build one at a time so the builders' inherited output does not interleave */
  for (const target of plan.targets) {
    results.push(await buildTargetExecutable(ctx, target))
  }
  /* eslint-enable no-await-in-loop */

  return `Built ${plan.targets.length} executable${plan.targets.length === 1 ? '' : 's'}:\n${results.join('\n')}`
}

async function buildTargetExecutable (ctx: TargetBuildContext, target: ParsedTarget): Promise<string> {
  const embeddedNodeBin = await ensureNodeRuntime({
    buildRoot: ctx.buildRoot,
    version: ctx.nodeVersion,
    platform: target.platform,
    arch: target.arch,
    libc: target.libc,
  })

  const targetOutputDir = path.join(ctx.plan.outputDir, target.raw)
  await mkdir(targetOutputDir, { recursive: true })
  // A repo could symlink `dist-app/<target>` out of the project even when
  // `dist-app` itself is contained; re-check the real path before any
  // binary is written into it.
  if (!isWithinDir(targetOutputDir, ctx.dir)) {
    throw outputDirOutsideProjectError(targetOutputDir)
  }

  const outputFile = path.join(targetOutputDir, outputFileName(ctx.plan.outputName, target.platform))
  // Re-check the leaf path right before the build in case it became a
  // symlink after the upfront pass.
  rejectNonRegularOutputFile(outputFile)

  await runSeaBuilder(ctx.builderBin, {
    main: ctx.plan.resolvedEntry,
    output: outputFile,
    executable: embeddedNodeBin,
    disableExperimentalSEAWarning: true,
    useCodeCache: false,
    useSnapshot: false,
  })

  await adHocSignMacBinary(target, outputFile, ctx.dir)

  return `  ${target.raw}: ${outputFile} (Node.js ${ctx.nodeVersion})`
}

async function runSeaBuilder (builderBin: string, seaConfig: Record<string, unknown>): Promise<void> {
  // Write the SEA config into a fresh, unpredictable temp directory (0700
  // by default) rather than a predictable path under os.tmpdir(). Avoids
  // TOCTOU/symlink attacks on multi-user systems.
  const tmpConfigDir = await mkdtemp(path.join(os.tmpdir(), 'pnpm-pack-app-'))
  const configPath = path.join(tmpConfigDir, 'sea-config.json')
  await writeFile(configPath, JSON.stringify(seaConfig, null, 2), { flag: 'wx' })

  try {
    await execa(builderBin, ['--build-sea', configPath], { stdio: 'inherit' })
  } finally {
    await rm(tmpConfigDir, { recursive: true, force: true }).catch(() => {})
  }
}
