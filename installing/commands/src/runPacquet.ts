import { spawn } from 'node:child_process'
import fs from 'node:fs'
import { createRequire } from 'node:module'
import path from 'node:path'
import readline from 'node:readline'
import type { Writable } from 'node:stream'

import { PnpmError } from '@pnpm/error'
import { logger, streamParser } from '@pnpm/logger'

// The runtime `streamParser` is a `Transform` stream (split2 + JSON.parse).
// Its public typing only exposes `on`/`removeListener`, so we narrow to the
// writable side here to feed pacquet's NDJSON lines back through the same
// parser that `@pnpm/cli.default-reporter` listens on.
const streamParserWritable = streamParser as unknown as Writable

export interface MakeRunPacquetOpts {
  lockfileDir: string
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
   * The user's original `pnpm` argv (`process.argv.slice(2)`). Not
   * forwarded to pacquet — we only inspect it to warn about flags
   * pacquet won't see.
   */
  argv: string[]
}

/**
 * Build the install-engine callback `mutateModules` invokes when
 * `configDependencies` declares pacquet. Returns `undefined` when no
 * pacquet binary is on disk — the caller falls back to the JS path in
 * that case.
 *
 * The callback spawns the pacquet binary installed under
 * `node_modules/.pnpm-config/pacquet` and forwards the user's own
 * pnpm CLI flags. Pacquet's NDJSON stderr is parsed line-by-line and
 * the valid JSON records are re-emitted on pnpm's global
 * `streamParser` so `@pnpm/cli.default-reporter` renders pacquet's
 * events the same way it renders pnpm's own. Non-JSON stderr lines
 * (panic backtraces, unexpected diagnostics) are forwarded to the
 * real stderr verbatim so they reach the user.
 */
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
}

export function makeRunPacquet (opts: MakeRunPacquetOpts): (callOpts?: RunPacquetCallOpts) => Promise<void> {
  return async (callOpts) => {
    const pacquetBin = resolvePacquetBin(opts.lockfileDir, opts.packageName)
    // Always the same fixed args. We don't forward pnpm's CLI flags
    // even though pacquet's `install` subcommand mirrors most of them:
    // pnpm has commands like `add` and `update` that carry flags
    // pacquet's `install` doesn't recognize (e.g., `--save-dev`,
    // `--save-peer`), and clap would reject them. The settings users
    // care about live in `pnpm-workspace.yaml` / `.npmrc`, which
    // pacquet reads on its own.
    const args = ['--reporter=ndjson', 'install', '--frozen-lockfile']
    const droppedFlags = collectDroppedFlags(opts.argv)
    if (droppedFlags.length > 0) {
      logger.warn({
        message: `The following CLI flags are not forwarded to pacquet and may not be honored: ${droppedFlags.join(' ')}. Move the equivalent settings into pnpm-workspace.yaml (or .npmrc for auth/registry) if pacquet needs them.`,
        prefix: opts.lockfileDir,
      })
    }
    logger.info({ message: 'Delegating install to pacquet (configured via configDependencies)', prefix: opts.lockfileDir })
    const child = spawn(pacquetBin, args, {
      cwd: opts.lockfileDir,
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
  return createRequire(pacquetPkg).resolve(`@pacquet/${process.platform}-${process.arch}/pacquet${ext}`)
}

/**
 * Pull the CLI flags out of pnpm's argv so we can warn about them
 * before pacquet runs. We don't forward any of them — pacquet always
 * gets `install --frozen-lockfile --reporter=ndjson` — but most are
 * handled by pnpm itself before delegation (`--save-dev` rewrites
 * `package.json`, `--filter` selects projects, etc.) so listing them
 * to the user makes the "not forwarded" surface concrete.
 *
 * Flags we explicitly emit ourselves (`--frozen-lockfile`,
 * `--reporter=ndjson`) are filtered out: they're honored, so warning
 * about them would be misleading. `--config.*` is filtered too —
 * those configure pnpm's runtime and aren't intended for the install
 * engine.
 */
function collectDroppedFlags (argv: string[]): string[] {
  return argv.filter((arg) => {
    if (!arg.startsWith('-')) return false
    if (arg === '--frozen-lockfile' || arg === '--reporter=ndjson') return false
    if (arg.startsWith('--config.')) return false
    return true
  })
}
