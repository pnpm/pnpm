import { spawn } from 'node:child_process'
import fs from 'node:fs'
import { createRequire } from 'node:module'
import path from 'node:path'
import readline from 'node:readline'
import type { Writable } from 'node:stream'

import { PnpmError } from '@pnpm/error'
import { logger, streamParser } from '@pnpm/logger'
import chalk from 'chalk'
import { familySync as getLibcFamilySync, MUSL } from 'detect-libc'

// The runtime `streamParser` is a `Transform` stream (split2 + JSON.parse).
// Its public typing only exposes `on`/`removeListener`, so we narrow to the
// writable side here to feed pacquet's NDJSON lines back through the same
// parser that `@pnpm/cli.default-reporter` listens on.
const streamParserWritable = streamParser as unknown as Writable

export interface MakeRunPacquetOpts {
  lockfileDir: string
  /**
   * Effective pnpm config value. Forwarded through `PNPM_CONFIG_*` so
   * pacquet writes the same `.modules.yaml` and virtual-store paths as
   * the pnpm process that delegated to it, including Windows' shorter
   * default.
   */
  virtualStoreDirMaxLength: number
  /**
   * Which `configDependencies` entry installed pacquet: either the
   * original unscoped `pacquet` or the official scoped
   * `@pnpm/pacquet` mirror. Drives the directory we look in under
   * `node_modules/.pnpm-config/<packageName>/`. Both packages ship
   * the same shim and the same `@pacquet/<plat>-<arch>` binary
   * sub-packages, so the rest of the lookup is identical.
   */
  packageName: 'pacquet' | '@pnpm/pacquet'
  /**
   * The parsed pnpm argv from `@pnpm/cli.parse-cli-args` — `original`
   * preserves the user's exact tokens (so `--key=value` stays joined,
   * which pacquet's `--config.<key>=<value>` parser requires), and
   * `remain` lists the positionals (the `install`/`i` command token
   * among them). When `isInstallCommand` is true we forward
   * `original` minus positionals to pacquet's own `install`
   * subcommand; otherwise we only inspect it to warn about flags
   * pacquet won't see.
   */
  argv: {
    original: string[]
    remain: string[]
  }
  /**
   * `true` when the user invoked `pnpm install` (or `pnpm i`). Gates
   * flag forwarding: pacquet's `install` subcommand mirrors pnpm's
   * surface closely enough that the user's flags are safe to pass
   * along on that command, but not from `add`/`update`/`dedupe` (whose
   * own flag surface doesn't line up with pacquet's `install`).
   */
  isInstallCommand: boolean
}

/** Args the deps-installer passes per pacquet invocation. */
export interface RunPacquetCallOpts {
  /**
   * `true` when pnpm has already run a lockfileOnly resolve pass and
   * the reporter has already accumulated one `pnpm:progress
   * status:resolved` per package. Pacquet's own `resolved` events
   * (emitted for wire-format parity as it walks the lockfile) are
   * dropped on the way back through the reader so the reporter
   * doesn't double-count. The frozen-install path passes `false`:
   * pnpm did no resolution there, so pacquet's events are the only
   * source.
   */
  filterResolvedProgress?: boolean
  /**
   * `true` to let pacquet perform the resolution itself rather than
   * materialize an already-resolved lockfile. Drops the injected
   * `--frozen-lockfile`, so pacquet resolves the manifests, writes
   * `pnpm-lock.yaml`, and materializes in a single pass. Only valid
   * when {@link PacquetEngine.supportsResolution} is `true` (pacquet
   * >= 0.11.7).
   */
  resolve?: boolean
}

/**
 * Handle to the pacquet install engine: its capabilities plus the
 * callback `mutateModules` invokes to run it.
 */
export interface PacquetEngine {
  /**
   * `true` when the installed pacquet is new enough (>= 0.11.7) to
   * perform dependency resolution itself. When `false`, pacquet can
   * only materialize an already-resolved lockfile, so the deps-installer
   * runs its own resolve pass first and hands the written lockfile to
   * pacquet.
   */
  supportsResolution: boolean
  run: (callOpts?: RunPacquetCallOpts) => Promise<void>
}

/**
 * Build the pacquet install engine `mutateModules` delegates to when
 * `configDependencies` declares pacquet.
 *
 * `run` spawns the pacquet binary installed under
 * `node_modules/.pnpm-config/pacquet`. From `pnpm install`/`pnpm i` it
 * forwards the user's own pnpm CLI flags to pacquet's `install`
 * subcommand; from `add`/`update`/`dedupe` it doesn't forward (warning
 * instead). Pacquet's NDJSON stderr is parsed line-by-line and the
 * valid JSON records are re-emitted on pnpm's global `streamParser` so
 * `@pnpm/cli.default-reporter` renders pacquet's events the same way it
 * renders pnpm's own. Non-JSON stderr lines (panic backtraces,
 * unexpected diagnostics) are forwarded to the real stderr verbatim so
 * they reach the user.
 */
export function makeRunPacquet (opts: MakeRunPacquetOpts): PacquetEngine {
  return {
    supportsResolution: pacquetSupportsResolution(resolvePacquetVersion(opts.lockfileDir, opts.packageName)),
    run: makeRun(opts),
  }
}

function makeRun (opts: MakeRunPacquetOpts): (callOpts?: RunPacquetCallOpts) => Promise<void> {
  return async (callOpts) => {
    const pacquetBin = resolvePacquetBin(opts.lockfileDir, opts.packageName)
    // From `pnpm install`/`pnpm i` we forward the user's flags through to
    // pacquet's own `install` subcommand verbatim — pacquet mirrors pnpm's
    // surface closely enough on that command that they're safe to pass
    // along. From `add`/`update`/`dedupe` we don't forward anything: those
    // commands carry flags pacquet's `install` doesn't recognize
    // (`--save-dev`, `--save-peer`, etc.) which clap would reject.
    const forwardedFlags = opts.isInstallCommand ? collectForwardedFlags(opts.argv) : []
    // In resolve mode pacquet does the resolution itself, so it must not
    // be pinned to the existing lockfile — drop both injected flags.
    //
    // Otherwise (frozen materialization) inject `--frozen-lockfile` plus
    // `--ignore-manifest-check`. The latter tells pacquet to skip its
    // per-importer `package.json` ↔ `pnpm-lock.yaml` freshness gate:
    // pnpm just resolved and wrote the lockfile itself; on `pnpm up` /
    // `add` / `remove` the manifest on disk is still the pre-mutation
    // copy (pnpm writes it after `mutateModules` returns), so pacquet's
    // own check would always fire here. See
    // https://github.com/pnpm/pnpm/issues/11797. The flag is narrow
    // (only the manifest check); settings drift like `overrides` is
    // still enforced and was already re-validated by pnpm.
    const frozenArgs = callOpts?.resolve === true ? [] : ['--frozen-lockfile', '--ignore-manifest-check']
    const args = ['--reporter=ndjson', 'install', ...frozenArgs, ...forwardedFlags]
    const droppedFlags = opts.isInstallCommand ? [] : collectDroppedFlags(opts.argv)
    if (droppedFlags.length > 0) {
      logger.warn({
        message: `The following CLI flags are not forwarded to pacquet and may not be honored: ${droppedFlags.join(' ')}. Move the equivalent settings into pnpm-workspace.yaml (or .npmrc for auth/registry) if pacquet needs them.`,
        prefix: opts.lockfileDir,
      })
    }
    // Banner so users can tell at a glance their install is going
    // through the Rust engine rather than the JS path. Chalk is the
    // same dependency the default reporter uses for the "+ pkg
    // version" summary, so colorization respects the user's TTY
    // settings consistently.
    const banner = [
      chalk.magentaBright('▶ Using pacquet for this install'),
      chalk.gray('  pacquet is pnpm\'s Rust install engine (preview); declared in configDependencies.'),
    ].join('\n')
    logger.info({ message: banner, prefix: opts.lockfileDir })
    const child = spawn(pacquetBin, args, {
      cwd: opts.lockfileDir,
      env: makePacquetEnv(opts),
      stdio: ['ignore', 'inherit', 'pipe'],
    })
    const filterResolved = callOpts?.filterResolvedProgress === true
    const rl = readline.createInterface({ input: child.stderr!, crlfDelay: Infinity })
    rl.on('line', (line) => {
      if (!line) return
      let parsed: unknown
      try {
        parsed = JSON.parse(line)
      } catch {
        process.stderr.write(`${line}\n`)
        return
      }
      if (
        filterResolved &&
        typeof parsed === 'object' && parsed !== null &&
        (parsed as { name?: string }).name === 'pnpm:progress' &&
        (parsed as { status?: string }).status === 'resolved'
      ) {
        return
      }
      streamParserWritable.write(`${line}\n`)
    })
    await new Promise<void>((resolve, reject) => {
      child.once('error', reject)
      child.once('close', (code) => {
        rl.close()
        if (code === 0) {
          resolve()
          return
        }
        reject(new PnpmError('PACQUET_INSTALL_FAILED', `pacquet exited with code ${code ?? 'null'}`))
      })
    })
  }
}

function makePacquetEnv (opts: MakeRunPacquetOpts): NodeJS.ProcessEnv {
  const env = { ...process.env }
  for (const key of Object.keys(env)) {
    if (key.toLowerCase() === 'pnpm_config_virtual_store_dir_max_length') {
      delete env[key]
    }
  }
  env.PNPM_CONFIG_VIRTUAL_STORE_DIR_MAX_LENGTH = String(opts.virtualStoreDirMaxLength)
  return env
}

/**
 * Path of the platform-specific native pacquet binary for the host. The
 * pacquet npm package ships a Node wrapper at `bin/pacquet` that uses
 * `require.resolve('@pacquet/<platform>-<arch>/pacquet[.exe]')` to find
 * the binary — so the platform package lands as a *sibling* of pacquet,
 * not inside its own `node_modules` (pacquet's own `node_modules` is
 * empty after configDependencies install). Use Node's resolver rooted
 * at pacquet's own `package.json` so we follow the same path the
 * wrapper would have.
 *
 * The `realpathSync` is required: `.pnpm-config/pacquet` is a symlink
 * into the global virtual store, and Node's `createRequire` builds its
 * search paths from the *literal* ancestors of the path it's given —
 * it won't follow the symlink up into the store dir where the
 * `@pacquet/<plat>-<arch>` sibling actually lives.
 */
function resolvePacquetBin (lockfileDir: string, packageName: 'pacquet' | '@pnpm/pacquet'): string {
  const ext = process.platform === 'win32' ? '.exe' : ''
  const pacquetPkg = fs.realpathSync(path.join(lockfileDir, 'node_modules/.pnpm-config', packageName, 'package.json'))
  return createRequire(pacquetPkg).resolve(`${pacquetPlatformPkgName()}/pacquet${ext}`)
}

/**
 * Name of the `@pacquet/<platform>-<arch>[-musl]` package that holds the
 * native pacquet binary for the host. On linux the binary packages are
 * split by libc and only the matching one is installed, so spawning and
 * signature verification must agree on this exact name.
 */
export function pacquetPlatformPkgName (): string {
  const libc = process.platform === 'linux' && getLibcFamilySync() === MUSL ? '-musl' : ''
  return `@pacquet/${process.platform}-${process.arch}${libc}`
}

/**
 * Read the installed pacquet's version from its `package.json` under
 * `node_modules/.pnpm-config/<packageName>`. Returns `undefined` if it
 * can't be read — callers treat that as "assume the older,
 * materialization-only pacquet" so a missing/garbled manifest degrades
 * to the safe path rather than failing the install.
 */
function resolvePacquetVersion (lockfileDir: string, packageName: 'pacquet' | '@pnpm/pacquet'): string | undefined {
  try {
    const pacquetPkg = fs.realpathSync(path.join(lockfileDir, 'node_modules/.pnpm-config', packageName, 'package.json'))
    const { version } = JSON.parse(fs.readFileSync(pacquetPkg, 'utf8')) as { version?: string }
    return version
  } catch {
    return undefined
  }
}

/**
 * pacquet gained full resolving installs in 0.11.7; earlier releases
 * stay on pnpm's resolve-then-materialize path. Pre-release builds of
 * 0.11.7 (e.g. `0.11.7-rc.1`) count as supporting it.
 */
function pacquetSupportsResolution (version: string | undefined): boolean {
  if (version == null) return false
  const [major, minor, patch] = version.split('.', 3).map((part) => parseInt(part, 10))
  if (Number.isNaN(major) || Number.isNaN(minor) || Number.isNaN(patch)) return false
  return major > 0 || (major === 0 && (minor > 11 || (minor === 11 && patch >= 7)))
}

/**
 * From `pnpm install`/`pnpm i`, return everything in argv that should
 * ride along to pacquet's own `install` subcommand. Drops the
 * positionals nopt classified (`install` / `i`, plus anything users
 * typed positionally) since pacquet's `install` doesn't accept any —
 * leaving them in produces `error: unexpected argument 'install'
 * found`. Pacquet's clap parser walks the same `--prod`, `--dev`,
 * `--no-optional`, `--no-runtime`, `--node-linker`, `--offline`,
 * `--prefer-offline`, `--cpu`, `--os`, `--libc`, `--frozen-lockfile`
 * surface pnpm itself accepts on `install`, so the flags don't need
 * reshaping.
 *
 * Flags we always inject ourselves (`--frozen-lockfile`,
 * `--ignore-manifest-check`) are dropped in every form the user can
 * type them — positive (`--frozen-lockfile`), negated
 * (`--no-frozen-lockfile`), and any `=value` form. Pacquet's clap
 * defines these as plain `#[clap(long)] bool` flags, so a duplicate
 * `--frozen-lockfile` or a conflicting `--no-frozen-lockfile`
 * crashes the parser with "used multiple times" / "unexpected
 * argument". The user's `--no-frozen-lockfile` intent is already
 * honored upstream (pnpm did a fresh resolve before delegating);
 * pacquet's role here is just lockfile-driven materialization.
 *
 * `--reporter` is stripped in any form (`--reporter=foo`,
 * `--reporter foo`): pacquet's `reporter` is a clap value option
 * with last-value-wins semantics, so a user-supplied value would
 * override our `--reporter=ndjson` and break the
 * NDJSON-to-streamParser plumbing the default reporter relies on.
 */
function collectForwardedFlags (argv: { original: string[], remain: string[] }): string[] {
  const result: string[] = []
  // `argv.remain` is the ordered subsequence of positionals nopt
  // extracted from `original`. Match by index rather than by value so
  // an option's value that happens to equal a positional (e.g.
  // `--node-linker install`) isn't mistaken for the positional itself.
  let positionalIdx = 0
  for (let i = 0; i < argv.original.length; i++) {
    const arg = argv.original[i]
    if (positionalIdx < argv.remain.length && arg === argv.remain[positionalIdx]) {
      positionalIdx++
      continue
    }
    if (isAlwaysInjected(arg)) continue
    if (arg.startsWith('--reporter=')) continue
    if (arg === '--reporter') {
      // Consume the next token as the reporter value (`--reporter foo`).
      i++
      continue
    }
    result.push(arg)
  }
  return result
}

const ALWAYS_INJECTED_FLAGS = ['frozen-lockfile', 'ignore-manifest-check'] as const

function isAlwaysInjected (arg: string): boolean {
  for (const name of ALWAYS_INJECTED_FLAGS) {
    if (arg === `--${name}` || arg === `--no-${name}`) return true
    if (arg.startsWith(`--${name}=`) || arg.startsWith(`--no-${name}=`)) return true
  }
  return false
}

/**
 * From a non-install command (`add`, `update`, `dedupe`, ...), pull the
 * CLI flags out of pnpm's argv so we can warn that pacquet won't see
 * them. They're still handled by pnpm itself before delegation
 * (`--save-dev` rewrites `package.json`, `--filter` selects projects,
 * etc.) so listing them to the user makes the "not forwarded" surface
 * concrete.
 *
 * Flags pnpm itself honors before delegation are filtered out —
 * warning about them would be misleading: `--frozen-lockfile` and
 * `--ignore-manifest-check` in every shape (positive / negated /
 * `=value`); `--reporter` in every shape (`--reporter=foo`,
 * `--reporter foo`); and `--config.*` (configures pnpm's runtime,
 * not the install engine).
 */
function collectDroppedFlags (argv: { original: string[] }): string[] {
  const result: string[] = []
  for (let i = 0; i < argv.original.length; i++) {
    const arg = argv.original[i]
    if (!arg.startsWith('-')) continue
    if (isAlwaysInjected(arg)) continue
    if (arg.startsWith('--config.')) continue
    if (arg.startsWith('--reporter=')) continue
    if (arg === '--reporter') {
      i++
      continue
    }
    result.push(arg)
  }
  return result
}
