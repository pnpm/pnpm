import { spawn } from 'node:child_process'
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
   * The user's original `pnpm` argv (`process.argv.slice(2)`). Forwarded
   * to pacquet verbatim — except for argv[0], which is pnpm's
   * subcommand alias (`install` / `i`) and is always replaced with
   * `install` because pacquet doesn't share pnpm's aliases.
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
export function makeRunPacquet (opts: MakeRunPacquetOpts): () => Promise<void> {
  return async () => {
    const pacquetBin = path.join(opts.lockfileDir, 'node_modules/.pnpm-config/pacquet/bin/pacquet')
    const args = buildArgs(opts.argv)
    logger.info({ message: 'Delegating install to pacquet (configured via configDependencies)', prefix: opts.lockfileDir })
    // `pacquetBin` is the npm package's `bin/pacquet` Node wrapper
    // script (resolves `@pacquet/<platform>-<arch>/pacquet` and execs
    // it). Invoke via `process.execPath` so the launcher runs under
    // the same Node binary on every platform — direct spawn would
    // rely on shebang support, which Windows doesn't have.
    const child = spawn(process.execPath, [pacquetBin, ...args], {
      cwd: opts.lockfileDir,
      stdio: ['ignore', 'inherit', 'pipe'],
    })
    const rl = readline.createInterface({ input: child.stderr!, crlfDelay: Infinity })
    rl.on('line', (line) => {
      if (!line) return
      try {
        JSON.parse(line)
      } catch {
        process.stderr.write(`${line}\n`)
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
 * Translate pnpm's invocation argv into the pacquet command line. The
 * user's flags pass through unchanged — pacquet's CLI surface mirrors
 * pnpm's by design — with two adjustments:
 *
 *   1. argv[0] (pnpm's subcommand alias, e.g. `i`) is dropped and
 *      replaced with the literal `install`. Pacquet doesn't share
 *      pnpm's aliases.
 *   2. `--frozen-lockfile` is appended if absent. We only delegate
 *      from `tryFrozenInstall`, which has already established the
 *      lockfile is good — but the user may have arrived via the
 *      optimistic `preferFrozenLockfile` path without passing the
 *      flag explicitly, in which case pacquet still needs it (its
 *      only supported install mode is frozen).
 *
 * `--reporter=ndjson` is always prepended — pacquet's default is
 * `silent`, so without this the reporter pipe would see nothing.
 */
function buildArgs (argv: string[]): string[] {
  const forwarded = argv.slice(1)
  const args = ['--reporter=ndjson', 'install']
  if (!forwarded.includes('--frozen-lockfile')) args.push('--frozen-lockfile')
  args.push(...forwarded)
  return args
}
