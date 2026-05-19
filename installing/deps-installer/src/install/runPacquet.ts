import { spawn } from 'node:child_process'
import path from 'node:path'
import readline from 'node:readline'
import type { Writable } from 'node:stream'

import { PnpmError } from '@pnpm/error'
import { streamParser } from '@pnpm/logger'

// The runtime `streamParser` is a `Transform` stream (split2 + JSON.parse).
// Its public typing only exposes `on`/`removeListener`, so we narrow to the
// writable side here to feed pacquet's NDJSON lines back through the same
// parser that `@pnpm/cli.default-reporter` listens on.
const streamParserWritable = streamParser as unknown as Writable

export interface RunPacquetOpts {
  lockfileDir: string
}

/**
 * Spawn the pacquet binary installed via `configDependencies` to perform
 * the install. Pacquet emits NDJSON log events on stderr that match the
 * `pnpm:<channel>` shape `@pnpm/cli.default-reporter` already parses, so
 * each line is forwarded to the global `streamParser` and rendered by the
 * same reporter that handles pnpm's own events. Non-JSON stderr lines
 * (panic backtraces, unexpected diagnostics) are forwarded to the real
 * stderr verbatim so they reach the user.
 *
 * `--frozen-lockfile` is always passed: pacquet only implements the
 * frozen-install path, and we only delegate inside `tryFrozenInstall`,
 * which already established the lockfile is usable.
 */
export async function runPacquet (opts: RunPacquetOpts): Promise<void> {
  const pacquetBin = path.join(opts.lockfileDir, 'node_modules/.pnpm-config/pacquet/bin/pacquet')
  // Pacquet defaults to `--reporter=silent`. Asking for `ndjson` is what
  // makes it emit the `pnpm:*` log events the loop below forwards.
  const args = ['--reporter=ndjson', 'install', '--frozen-lockfile']
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
