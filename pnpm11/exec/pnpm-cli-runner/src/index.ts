import path from 'node:path'

import { detectIfCurrentPkgIsExecutable } from '@pnpm/cli.meta'
import { sync as execSync } from 'execa'

/**
 * The names `process.argv[1]` takes when pnpm itself is the entry. Both
 * spellings occur because `process.argv[1]` keeps the name the process was
 * launched through: npm links its bins as symlinks, so it is `pn` or `pnpm`,
 * while pnpm's own shims and Corepack name the target, so it is `pnpm.mjs` or
 * `pnpm.cjs`.
 *
 * The list cannot be dropped. `runPnpmCli` also runs inside processes that
 * merely import pnpm's packages, such as a Jest host, whose `process.argv[1]`
 * must not be re-executed. `pnpx` and `pnx` are absent for the mirror image of
 * that reason: that entry rewrites `process.argv` to prepend `dlx`, so
 * re-running it would turn `add` into `dlx add`.
 */
const PNPM_ENTRY_SCRIPTS = new Set(['pnpm', 'pn', 'pnpm.cjs', 'pnpm.mjs'])

export interface RunPnpmCliOptions {
  cwd: string
  reporter?: string
}

export function runPnpmCli (command: string[], { cwd, reporter }: RunPnpmCliOptions): void {
  const execOpts = {
    cwd,
    stdio: 'inherit' as const,
  }
  const cliCommand = reporter ? [...command, `--reporter=${reporter}`] : command
  const entryScript = process.argv[1]
  // Re-invoke the pnpm that is running now, so the child runs the same version.
  // The `@pnpm/exe` single-file build is its own executable; every other
  // install method runs one of the entry scripts above. Anything else has to
  // guess and take whichever pnpm is on PATH.
  if (detectIfCurrentPkgIsExecutable()) {
    execSync(process.execPath, cliCommand, execOpts)
  } else if (entryScript && PNPM_ENTRY_SCRIPTS.has(path.basename(entryScript).toLowerCase())) {
    execSync(process.execPath, [entryScript, ...cliCommand], execOpts)
  } else {
    execSync('pnpm', cliCommand, execOpts)
  }
}
