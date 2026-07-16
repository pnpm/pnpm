import { spawnSync } from 'node:child_process'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import util from 'node:util'

import { detectIfCurrentPkgIsExecutable, getCurrentPackageName, isExecutedByCorepack, packageManager } from '@pnpm/cli.meta'
import { docsUrl } from '@pnpm/cli.utils'
import { types as allTypes } from '@pnpm/config.reader'
import chalk from 'chalk'
import { pick } from 'ramda'
import { renderHelp } from 'render-help'

export const commandNames = ['doctor']

/**
 * `pnpm doctor` runs read-only diagnostics that predict whether an install
 * will work on this machine and how fast it will be, plus one live check —
 * an offline `file:` install — that exercises the resolve → store → link
 * path end to end. It runs the same checks whether a user invokes it or the
 * release pipeline does before promoting a version, so the release gate tests
 * exactly what ships.
 */
export const skipPackageManagerCheck = true

export function rcOptionsTypes (): Record<string, unknown> {
  return pick(['offline'], allTypes)
}

export function cliOptionsTypes (): Record<string, unknown> {
  return {
    ...rcOptionsTypes(),
    json: Boolean,
    benchmark: Boolean,
  }
}

export function help (): string {
  return renderHelp({
    description: 'Run diagnostics on the pnpm installation and environment.',
    descriptionLists: [
      {
        title: 'Options',
        list: [
          {
            description: 'Skip checks that need network access',
            name: '--offline',
          },
          {
            description: 'Also time filesystem and install operations',
            name: '--benchmark',
          },
          {
            description: 'Report the results as JSON',
            name: '--json',
          },
        ],
      },
    ],
    url: docsUrl('doctor'),
    usages: ['pnpm doctor [--offline] [--benchmark] [--json]'],
  })
}

type CheckStatus = 'pass' | 'warn' | 'fail'

export interface CheckResult {
  title: string
  status: CheckStatus
  detail?: string
  /** A concrete next step shown when the check does not pass. */
  fix?: string
  durationMs?: number
}

export interface DoctorCommandOptions {
  dir: string
  cacheDir: string
  pnpmHomeDir: string
  globalBinDir?: string
  storeDir?: string
  registries?: Record<string, string>
  offline?: boolean
  json?: boolean
  benchmark?: boolean
  /**
   * The argv used to re-invoke pnpm for the install smoke test. Defaults to
   * the running binary; tests point it at a built entry instead.
   */
  pnpmCommand?: string[]
}

const DEFAULT_REGISTRY = 'https://registry.npmjs.org/'

export async function handler (opts: DoctorCommandOptions): Promise<{ output: string, exitCode: number }> {
  const pnpmCommand = opts.pnpmCommand ?? resolveSelfCommand()

  const checks: CheckResult[] = [
    checkVersions(),
    checkInstallMethod(),
    await checkGlobalBinDir(opts),
    await checkWritableDir('Cache directory', opts.cacheDir),
    ...(opts.storeDir ? [await checkWritableDir('Store directory', opts.storeDir)] : []),
    await checkFilesystemCapabilities(opts),
    await checkConnectivity(opts),
    await checkInstallSmokeTest(pnpmCommand, opts),
  ]

  const exitCode = checks.some((check) => check.status === 'fail') ? 1 : 0

  if (opts.json) {
    return { output: JSON.stringify({ checks }, undefined, 2), exitCode }
  }
  return { output: renderReport(checks), exitCode }
}

function checkVersions (): CheckResult {
  return {
    title: 'Versions',
    status: 'pass',
    detail: `pnpm ${packageManager.version}, Node.js ${process.versions.node}`,
  }
}

function checkInstallMethod (): CheckResult {
  const wrapper = getCurrentPackageName()
  if (isExecutedByCorepack()) {
    return {
      title: 'Install method',
      status: 'warn',
      detail: `${wrapper}, run by Corepack`,
      fix: 'Corepack manages the pnpm version itself; "pnpm self-update" is unavailable under it.',
    }
  }
  return { title: 'Install method', status: 'pass', detail: wrapper }
}

/**
 * Check the global executables directory — where `pnpm setup` links binaries
 * and which must be on PATH for them to run. Not `opts.bin`, which outside
 * `--global` is the local `node_modules/.bin` and is never on PATH. The layout
 * moved between majors (v10 links into PNPM_HOME directly, v11 into
 * PNPM_HOME/bin), so accept whichever candidate PATH actually contains.
 */
async function checkGlobalBinDir (opts: DoctorCommandOptions): Promise<CheckResult> {
  const title = 'Global bin directory'
  const candidates = [
    opts.globalBinDir,
    path.join(opts.pnpmHomeDir, 'bin'),
    opts.pnpmHomeDir,
  ].filter((dir): dir is string => dir != null && dir !== '')

  const pathEnv = readPathEnv(process.env)
  if (pathEnv == null) {
    return { title, status: 'warn', detail: 'the PATH environment variable is not set' }
  }

  const inPath = await Promise.all(candidates.map((candidate) => dirIsInPath(candidate, pathEnv)))
  const binDir = candidates.find((_, index) => inPath[index])
  if (binDir == null) {
    return {
      title,
      status: 'warn',
      detail: `${candidates[0]} is not in PATH`,
      fix: 'Run "pnpm setup" to add it to your shell configuration.',
    }
  }
  if (!canWriteToDir(binDir)) {
    return {
      title,
      status: 'fail',
      detail: `no write access to ${binDir}`,
      fix: 'Run "pnpm setup", or fix the directory permissions.',
    }
  }
  return { title, status: 'pass', detail: binDir }
}

async function checkWritableDir (title: string, dir: string): Promise<CheckResult> {
  if (!canWriteToDir(dir)) {
    return {
      title,
      status: 'fail',
      detail: `no write access to ${dir}`,
      fix: 'Fix the directory permissions or point the setting at a writable path.',
    }
  }
  return { title, status: 'pass', detail: dir }
}

/**
 * Probe which link strategies work from the store's volume, since that is
 * what determines how packages land in `node_modules` and how fast an install
 * is: a reflink (copy-on-write) or hardlink is near-free, a plain copy is not.
 */
async function checkFilesystemCapabilities (opts: DoctorCommandOptions): Promise<CheckResult> {
  const title = 'Filesystem'
  const probeParent = opts.storeDir ?? opts.cacheDir
  let probeDir: string
  try {
    probeDir = await fs.promises.mkdtemp(path.join(probeParent, '.pnpm-doctor-'))
  } catch {
    probeDir = await fs.promises.mkdtemp(path.join(os.tmpdir(), 'pnpm-doctor-'))
  }
  const started = Date.now()
  try {
    const capabilities = await probeLinkCapabilities(probeDir)
    const available = Object.entries(capabilities).filter(([, ok]) => ok).map(([name]) => name)
    const status: CheckStatus = capabilities.reflink || capabilities.hardlink ? 'pass' : 'warn'
    return {
      title,
      status,
      detail: available.length > 0 ? `available: ${available.join(', ')}` : 'only copying is available',
      fix: status === 'warn'
        ? 'Neither reflink nor hardlink works between the store and this project; installs will copy files and be slower. Put the store on the same filesystem as your projects.'
        : undefined,
      durationMs: opts.benchmark ? Date.now() - started : undefined,
    }
  } finally {
    await fs.promises.rm(probeDir, { recursive: true, force: true })
  }
}

async function checkConnectivity (opts: DoctorCommandOptions): Promise<CheckResult> {
  const title = 'Registry connectivity'
  if (opts.offline) {
    return { title, status: 'pass', detail: 'skipped (--offline)' }
  }
  const registry = opts.registries?.default ?? DEFAULT_REGISTRY
  const pingUrl = new URL('./-/ping?write=true', registry.endsWith('/') ? registry : `${registry}/`)
  const started = Date.now()
  try {
    const response = await fetch(pingUrl, { signal: AbortSignal.timeout(15_000) })
    if (!response.ok) {
      return {
        title,
        status: 'fail',
        detail: `${registry} responded ${response.status} ${response.statusText}`.trimEnd(),
        fix: 'Check your registry, proxy, and auth configuration.',
      }
    }
    return { title, status: 'pass', detail: `${registry} (${Date.now() - started}ms)` }
  } catch (err: unknown) {
    return {
      title,
      status: 'fail',
      detail: `could not reach ${registry}: ${util.types.isNativeError(err) ? err.message : String(err)}`,
      fix: 'Check your network, proxy, and registry configuration.',
    }
  }
}

/**
 * Install a throwaway package as a `file:` dependency, entirely offline, to
 * confirm the running binary can resolve, fetch into the store, and link a
 * dependency end to end. Catches both classes of broken release the CI gate
 * exists for: a binary that will not run at all, and one whose install path
 * crashes.
 */
export async function checkInstallSmokeTest (
  pnpmCommand: string[],
  opts: Pick<DoctorCommandOptions, 'benchmark'>
): Promise<CheckResult> {
  const title = 'Install smoke test'
  const base = await fs.promises.mkdtemp(path.join(os.tmpdir(), 'pnpm-doctor-install-'))
  const started = Date.now()
  try {
    const provider = path.join(base, 'provider')
    const consumer = path.join(base, 'consumer')
    await fs.promises.mkdir(provider, { recursive: true })
    await fs.promises.mkdir(consumer, { recursive: true })
    await fs.promises.writeFile(
      path.join(provider, 'package.json'),
      JSON.stringify({ name: 'pnpm-doctor-fixture', version: '0.0.0' })
    )
    await fs.promises.writeFile(
      path.join(consumer, 'package.json'),
      JSON.stringify({
        name: 'pnpm-doctor-consumer',
        version: '0.0.0',
        private: true,
        dependencies: { 'pnpm-doctor-fixture': 'file:../provider' },
      })
    )

    const [command, ...baseArgs] = pnpmCommand
    const args = [
      ...baseArgs,
      'install',
      '--offline',
      '--ignore-scripts',
      '--ignore-workspace',
      '--no-frozen-lockfile',
      `--store-dir=${path.join(base, 'store')}`,
      `--cache-dir=${path.join(base, 'cache')}`,
    ]
    const result = spawnSync(command, args, { cwd: consumer, encoding: 'utf8', timeout: 120_000 })

    if (result.status !== 0) {
      const stderr = (result.stderr ?? '').trim()
      return {
        title,
        status: 'fail',
        detail: `offline "file:" install failed${stderr ? `: ${lastLine(stderr)}` : ''}`,
        fix: 'Run "pnpm install" in a scratch project to see the full error.',
      }
    }
    const linked = path.join(consumer, 'node_modules', 'pnpm-doctor-fixture', 'package.json')
    if (!fs.existsSync(linked)) {
      return { title, status: 'fail', detail: 'install reported success but the dependency was not linked' }
    }
    return {
      title,
      status: 'pass',
      detail: 'offline "file:" install linked its dependency',
      durationMs: opts.benchmark ? Date.now() - started : undefined,
    }
  } finally {
    await fs.promises.rm(base, { recursive: true, force: true })
  }
}

function renderReport (checks: CheckResult[]): string {
  const lines = checks.map((check) => {
    const head = `${statusMark(check.status)} ${check.title}${check.detail ? `: ${check.detail}` : ''}`
    if (check.status === 'pass' || check.fix == null) {
      return check.durationMs != null ? `${head} (${check.durationMs}ms)` : head
    }
    return `${head}\n    ${chalk.dim(check.fix)}`
  })
  const failed = checks.filter((check) => check.status === 'fail').length
  const warned = checks.filter((check) => check.status === 'warn').length
  const summary = failed > 0
    ? chalk.red(`${failed} check(s) failed`)
    : warned > 0
      ? chalk.yellow(`All checks passed with ${warned} warning(s)`)
      : chalk.green('All checks passed')
  return `${lines.join('\n')}\n\n${summary}`
}

function statusMark (status: CheckStatus): string {
  switch (status) {
    case 'pass': return chalk.green('✓')
    case 'warn': return chalk.yellow('‼')
    case 'fail': return chalk.red('✗')
  }
}

/**
 * Re-invoke the pnpm that is running now: `node <entry>` for the bundled
 * package, or the executable itself for the `@pnpm/exe` single-file build,
 * whose `process.argv[1]` is the binary rather than a script.
 */
function resolveSelfCommand (): string[] {
  if (detectIfCurrentPkgIsExecutable()) return [process.execPath]
  const entry = process.argv[1]
  if (!entry) return [process.execPath]
  return [process.execPath, entry]
}

async function probeLinkCapabilities (dir: string): Promise<{ reflink: boolean, hardlink: boolean, symlink: boolean }> {
  const source = path.join(dir, 'source')
  await fs.promises.writeFile(source, 'pnpm-doctor')
  return {
    reflink: await canLink(() => fs.promises.copyFile(source, path.join(dir, 'reflink'), fs.constants.COPYFILE_FICLONE_FORCE)),
    hardlink: await canLink(() => fs.promises.link(source, path.join(dir, 'hardlink'))),
    symlink: await canLink(() => fs.promises.symlink(source, path.join(dir, 'symlink'))),
  }
}

async function canLink (link: () => Promise<void>): Promise<boolean> {
  try {
    await link()
    return true
  } catch {
    return false
  }
}

function readPathEnv (env: NodeJS.ProcessEnv): string | undefined {
  if (process.platform !== 'win32') return env.PATH
  const key = Object.keys(env).find((name) => name.toUpperCase() === 'PATH')
  return key != null ? env[key] : undefined
}

async function dirIsInPath (dir: string, pathEnv: string): Promise<boolean> {
  const dirs = pathEnv.split(path.delimiter)
  if (dirs.some((entry) => areSameDir(dir, entry))) return true
  try {
    const real = await fs.promises.realpath(dir)
    return dirs.some((entry) => areSameDir(real, entry))
  } catch {
    return false
  }
}

const areSameDir = (a: string, b: string): boolean => a !== '' && b !== '' && path.relative(a, b) === ''

function canWriteToDir (dir: string): boolean {
  const probe = path.join(dir, `.pnpm-doctor-write-${process.pid}`)
  try {
    fs.writeFileSync(probe, '')
    fs.rmSync(probe, { force: true })
    return true
  } catch {
    return false
  }
}

function lastLine (text: string): string {
  const lines = text.split('\n').filter((line) => line.trim() !== '')
  return lines[lines.length - 1] ?? text
}
