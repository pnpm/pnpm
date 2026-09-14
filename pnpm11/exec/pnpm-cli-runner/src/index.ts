import fs from 'node:fs'
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
 * `pnpx` and `pnx` are absent on purpose: that entry rewrites `process.argv`
 * to prepend `dlx`, so re-running it would turn `add` into `dlx add`.
 */
const PNPM_ENTRY_SCRIPTS = new Set(['pnpm', 'pn', 'pnpm.cjs', 'pnpm.mjs'])

const PNPM_PACKAGE_NAMES = new Set(['pnpm', '@pnpm/exe'])

export interface RunPnpmCliOptions {
  cwd: string
  reporter?: string
}

export function runPnpmCli (command: string[], { cwd, reporter }: RunPnpmCliOptions): void {
  const cliCommand = reporter ? [...command, `--reporter=${reporter}`] : command
  const [executable, ...selfArgs] = resolvePnpmSelfCommand()
  execSync(executable, [...selfArgs, ...cliCommand], {
    cwd,
    stdio: 'inherit',
  })
}

/**
 * The command that re-invokes the pnpm running now, so a child runs the same
 * version: the executable itself for the `@pnpm/exe` single-file build, whose
 * `process.argv[1]` is the binary rather than a script, and `node <entry>` for
 * every other install method.
 *
 * Falls back to whichever pnpm is on `PATH` when this process is not pnpm.
 * `runPnpmCli` also runs inside processes that merely import pnpm's packages,
 * such as a Jest host, whose `process.argv[1]` must not be re-executed.
 */
export function resolvePnpmSelfCommand (): string[] {
  if (detectIfCurrentPkgIsExecutable()) return [process.execPath]
  const entryScript = process.argv[1]
  if (entryScript == null || !isPnpmEntryScript(entryScript)) return ['pnpm']
  return [process.execPath, entryScript]
}

/**
 * The name is only a hint, so confirm the script is pnpm's before handing it
 * the command. An unrelated script can carry one of these names, and the bare
 * ones are ordinary enough for another package to publish as a bin.
 */
function isPnpmEntryScript (entryScript: string): boolean {
  if (!PNPM_ENTRY_SCRIPTS.has(path.basename(entryScript).toLowerCase())) return false
  const owner = readOwningPackageName(entryScript)
  // Only a package that identifies itself as something other than pnpm is
  // rejected. An entry with no manifest above it, a copied bundle among them,
  // is still pnpm as far as anything here can tell, and refusing it would put
  // back the `PATH` lookup this exists to avoid.
  return owner == null || PNPM_PACKAGE_NAMES.has(owner)
}

/**
 * The `name` of the nearest package manifest above `entryScript`, or
 * `undefined` when there is none to read. Resolves symlinks first: an
 * npm-installed pnpm is reached through `node_modules/.bin`, where the nearest
 * manifest is the *consuming* project's rather than pnpm's own.
 */
function readOwningPackageName (entryScript: string): string | undefined {
  let dir: string
  try {
    dir = path.dirname(fs.realpathSync(entryScript))
  } catch {
    return undefined
  }
  for (;;) {
    let manifest: string
    try {
      manifest = fs.readFileSync(path.join(dir, 'package.json'), 'utf8')
    } catch {
      const parent = path.dirname(dir)
      if (parent === dir) return undefined
      dir = parent
      continue
    }
    try {
      return (JSON.parse(manifest) as { name?: string }).name
    } catch {
      return undefined
    }
  }
}
