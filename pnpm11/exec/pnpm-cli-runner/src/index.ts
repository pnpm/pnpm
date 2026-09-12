import path from 'node:path'

import { detectIfCurrentPkgIsExecutable } from '@pnpm/cli.meta'
import { sync as execSync } from 'execa'

/**
 * The entry scripts pnpm ships, by basename. `runPnpmCli` also runs inside
 * processes that merely import pnpm's packages, such as a Jest host, so
 * `process.argv[1]` may be someone else's script and cannot be re-executed on
 * faith. `pnpx` and `pnx` are deliberately absent: that entry rewrites
 * `process.argv` to prepend `dlx`, so re-running it would turn `add` into
 * `dlx add`.
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
