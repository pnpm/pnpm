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

interface OwningPackage {
  dir: string
  manifest: { name?: string, bin?: unknown }
}

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
 * Whether `entryScript` can be re-invoked as pnpm. The name alone is not
 * enough, since these are ordinary enough for another package to publish as a
 * bin, so a script carrying one is rejected when the package owning it claims
 * it as a bin of its own.
 *
 * Nothing stronger is asked of it. pnpm is distributed as a copyable bundle
 * and reached through shims and links, so a script that no package claims is
 * pnpm as far as anything here can tell, and refusing it would put back the
 * `PATH` lookup this exists to avoid.
 */
function isPnpmEntryScript (entryScript: string): boolean {
  if (!PNPM_ENTRY_SCRIPTS.has(path.basename(entryScript).toLowerCase())) return false
  let resolvedScript: string
  try {
    resolvedScript = fs.realpathSync(entryScript)
  } catch {
    return true
  }
  const owner = readOwningPackage(resolvedScript)
  if (owner == null || owner.manifest.name == null) return true
  if (PNPM_PACKAGE_NAMES.has(owner.manifest.name)) return true
  return !declaresBin(owner, resolvedScript)
}

/**
 * The nearest package above `resolvedScript`, or `undefined` when there is
 * none to read. Takes a resolved path because an npm-installed pnpm is reached
 * through `node_modules/.bin`, where the nearest package is the *consuming*
 * project rather than pnpm itself.
 */
function readOwningPackage (resolvedScript: string): OwningPackage | undefined {
  let dir = path.dirname(resolvedScript)
  for (;;) {
    let contents: string
    try {
      contents = fs.readFileSync(path.join(dir, 'package.json'), 'utf8')
    } catch {
      const parent = path.dirname(dir)
      if (parent === dir) return undefined
      dir = parent
      continue
    }
    try {
      return { dir, manifest: JSON.parse(contents) as OwningPackage['manifest'] }
    } catch {
      return undefined
    }
  }
}

function declaresBin ({ dir, manifest }: OwningPackage, resolvedScript: string): boolean {
  const { bin } = manifest
  const targets = typeof bin === 'string' ? [bin] : typeof bin === 'object' && bin !== null ? Object.values(bin) : []
  return targets.some((target) => typeof target === 'string' && path.resolve(dir, target) === resolvedScript)
}
