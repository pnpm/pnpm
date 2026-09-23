import fs from 'node:fs'
import path from 'node:path'

import which from 'which'

/**
 * Shim scripts (npm's `.cmd` files, Corepack's) name their target inside;
 * anything larger than this is a real executable, not a script.
 */
const SHIM_SCRIPT_MAX_SIZE = 64 * 1024

/**
 * How a `pnpm` outside the global bin directory got onto PATH, as far as its
 * location and, for a shim script, its contents tell.
 */
export type InstallOrigin = 'npm' | 'homebrew' | 'corepack' | 'volta' | 'scoop' | 'unknown'

/** A `pnpm` that a PATH lookup finds ahead of the one in the global bin directory. */
export interface ShadowingPnpm {
  executable: string
  origin: InstallOrigin
  /**
   * Whether the global bin directory is on PATH at all, behind `executable`.
   * When it is not, PATH has yet to be set up, and the shell will find the
   * right pnpm once the global bin directory is added ahead of `executable`.
   */
  globalBinOnPath: boolean
}

const REMOVAL_COMMANDS: Record<InstallOrigin, string | undefined> = {
  npm: 'npm uninstall -g pnpm',
  homebrew: 'brew uninstall pnpm',
  corepack: 'corepack disable pnpm',
  volta: 'volta uninstall pnpm',
  scoop: 'scoop uninstall pnpm',
  unknown: undefined,
}

const ORIGIN_DESCRIPTIONS: Record<InstallOrigin, string> = {
  npm: 'installed with npm',
  homebrew: 'installed with Homebrew',
  corepack: 'a Corepack shim',
  volta: 'installed with Volta',
  scoop: 'installed with Scoop',
  unknown: 'not installed by pnpm',
}

/**
 * The `pnpm` that a lookup through `pathEnv` finds ahead of the one in
 * `globalBin`, if any.
 *
 * `self-update` and `setup` install pnpm into the global bin directory. A
 * `pnpm` that another installer put ahead of that directory on PATH keeps
 * running after either command reports success, so every version they
 * install looks like it never took.
 *
 * A `pnpm` in `globalBin` itself, in its parent directory (the pnpm home
 * directory, which the v10 layout and CI actions link into directly), or a
 * symlink resolving into either, is pnpm's own and never shadows.
 */
export function findShadowingPnpm (globalBin: string, pathEnv: string | undefined): ShadowingPnpm | undefined {
  if (pathEnv == null) return undefined
  const [executable, ...others] = which.sync('pnpm', { all: true, nothrow: true, path: pathEnv }) ?? []
  if (executable == null || isOwnPnpm(executable, globalBin)) return undefined
  return {
    executable,
    origin: detectInstallOrigin(executable),
    globalBinOnPath: others.some((candidate) => isOwnPnpm(candidate, globalBin)),
  }
}

/**
 * The warning to print once a pnpm is linked into `globalBin` and
 * `shadowing` still answers to `pnpm`.
 */
export function renderShadowingPnpmWarning (shadowing: ShadowingPnpm, globalBin: string): string {
  const origin = ORIGIN_DESCRIPTIONS[shadowing.origin]
  const reorder = `move ${globalBin} ahead of ${path.dirname(shadowing.executable)} in PATH`
  const removal = REMOVAL_COMMANDS[shadowing.origin]
  const fix = removal != null ? `run "${removal}" or ${reorder}` : reorder
  if (shadowing.globalBinOnPath) {
    return `"pnpm" on PATH is ${shadowing.executable} (${origin}), which comes before ${globalBin}. ` +
      `Your shell keeps running that pnpm, not the one pnpm installed to ${globalBin}. ` +
      `To finish switching, ${fix}.`
  }
  return `"pnpm" on PATH is ${shadowing.executable} (${origin}), and ${globalBin} is not on PATH yet. ` +
    `Once a new shell adds it, it has to come first: ${fix}.`
}

export function detectInstallOrigin (executable: string): InstallOrigin {
  return originFromPath(realpathOrSelf(executable)) ??
    originFromPath(executable) ??
    originFromShimScript(executable) ??
    'unknown'
}

function originFromPath (executable: string): InstallOrigin | undefined {
  const components = executable.split(/[\\/]/).map((component) => component.toLowerCase())
  const follows = (first: string, second: string): boolean =>
    components.some((component, index) => component === first && components[index + 1] === second)
  if (follows('cellar', 'pnpm')) return 'homebrew'
  if (components.includes('corepack')) return 'corepack'
  if (components.includes('.volta')) return 'volta'
  if (components.includes('scoop')) return 'scoop'
  if (follows('node_modules', 'pnpm')) return 'npm'
  return undefined
}

function originFromShimScript (executable: string): InstallOrigin | undefined {
  let script: string
  try {
    if (fs.statSync(executable).size > SHIM_SCRIPT_MAX_SIZE) return undefined
    script = fs.readFileSync(executable, 'utf8')
  } catch {
    return undefined
  }
  if (script.includes('corepack')) return 'corepack'
  if (script.includes('node_modules/pnpm/') || script.includes('node_modules\\pnpm\\')) return 'npm'
  return undefined
}

function isOwnPnpm (executable: string, globalBin: string): boolean {
  const ownDirs = [globalBin, path.dirname(globalBin)]
  const dirs = [path.dirname(executable), path.dirname(realpathOrSelf(executable))]
  return dirs.some((dir) => ownDirs.some((own) => sameDir(dir, own)))
}

function sameDir (left: string, right: string): boolean {
  return path.relative(left, right) === '' || path.relative(realpathOrSelf(left), realpathOrSelf(right)) === ''
}

function realpathOrSelf (file: string): string {
  try {
    return fs.realpathSync(file)
  } catch {
    return file
  }
}
