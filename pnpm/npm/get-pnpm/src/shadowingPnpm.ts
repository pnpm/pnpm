import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

/**
 * Shim scripts (npm's `.cmd` files, Corepack's) name their target inside;
 * anything larger than this is a real executable, not a script.
 */
const SHIM_SCRIPT_MAX_SIZE = 64 * 1024

/**
 * How a `pnpm` outside the pnpm home directory got onto PATH, as far as its
 * location and, for a shim script, its contents tell.
 */
export type InstallOrigin = 'npm' | 'homebrew' | 'corepack' | 'volta' | 'scoop' | 'unknown'

/** A `pnpm` that a PATH lookup finds ahead of the one `pnpm setup` installed. */
export interface ShadowingPnpm {
  executable: string
  origin: InstallOrigin
  /**
   * Whether `$PNPM_HOME/bin` is on PATH at all, behind `executable`. When it
   * is not, PATH has yet to be set up, and the shell will find the right pnpm
   * once the directory is added ahead of `executable`.
   */
  binOnPath: boolean
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
 * Where `pnpm setup` installs: `PNPM_HOME`, or pnpm's default for the
 * platform, resolved the way pnpm resolves it.
 */
export function pnpmHomeDir (env: NodeJS.ProcessEnv = process.env, platform: NodeJS.Platform = process.platform): string {
  if (env.PNPM_HOME) return env.PNPM_HOME
  if (env.XDG_DATA_HOME) return path.join(env.XDG_DATA_HOME, 'pnpm')
  switch (platform) {
    case 'darwin': return path.join(os.homedir(), 'Library', 'pnpm')
    case 'win32': return env.LOCALAPPDATA ? path.join(env.LOCALAPPDATA, 'pnpm') : path.join(os.homedir(), '.pnpm')
    default: return path.join(os.homedir(), '.local', 'share', 'pnpm')
  }
}

/**
 * The `pnpm` that a lookup through `pathEnv` finds ahead of the one `pnpm
 * setup` installed under `pnpmHome`, if any.
 *
 * A `pnpm` from another installer (npm, Homebrew, Corepack, Volta, Scoop)
 * that sits ahead of `$PNPM_HOME/bin` on PATH keeps running after the
 * installer reports success, so every version it installs looks like it
 * never took.
 *
 * A `pnpm` in `$PNPM_HOME/bin`, in `$PNPM_HOME` itself (which the v10 layout
 * linked into directly), or a symlink resolving into either, is pnpm's own
 * and never shadows.
 */
export function findShadowingPnpm (
  pnpmHome: string,
  opts: { pathEnv: string | undefined, platform?: NodeJS.Platform, pathExt?: string | undefined }
): ShadowingPnpm | undefined {
  if (opts.pathEnv == null) return undefined
  const platform = opts.platform ?? process.platform
  const [executable, ...others] = lookUpAll(opts.pathEnv, platform, opts.pathExt)
  if (executable == null || isOwnPnpm(executable, pnpmHome)) return undefined
  return {
    executable,
    origin: detectInstallOrigin(executable),
    binOnPath: others.some((candidate) => isOwnPnpm(candidate, pnpmHome)),
  }
}

/**
 * The warning to print once `pnpm setup` installed under `pnpmHome` and
 * `shadowing` still answers to `pnpm`. Without `modifiedPath`, setup left the
 * shell config alone, so no new shell adds `$PNPM_HOME/bin` by itself.
 */
export function renderShadowingPnpmWarning (
  shadowing: ShadowingPnpm,
  pnpmHome: string,
  { modifiedPath = true }: { modifiedPath?: boolean } = {}
): string {
  const bin = path.join(pnpmHome, 'bin')
  const origin = ORIGIN_DESCRIPTIONS[shadowing.origin]
  const reorder = `move ${bin} ahead of ${path.dirname(shadowing.executable)} in PATH`
  const removal = REMOVAL_COMMANDS[shadowing.origin]
  const fix = removal != null ? `run "${removal}" or ${reorder}` : reorder
  if (shadowing.binOnPath) {
    return `Warning: "pnpm" on PATH is ${shadowing.executable} (${origin}), which comes before ${bin}.\n` +
      `Your shell keeps running that pnpm, not the one pnpm installed to ${bin}.\n` +
      `To finish switching, ${fix}.`
  }
  const when = modifiedPath ? 'Once a new shell adds it' : 'When you add it'
  return `Warning: "pnpm" on PATH is ${shadowing.executable} (${origin}), and ${bin} is not on PATH yet.\n` +
    `${when}, it has to come first: ${fix}.`
}

export function detectInstallOrigin (executable: string): InstallOrigin {
  return originFromPath(realpathOrSelf(executable)) ??
    originFromPath(executable) ??
    originFromShimScript(executable) ??
    'unknown'
}

/** Every `pnpm` on `pathEnv`, in lookup order, the way the shell would find them. */
function lookUpAll (pathEnv: string, platform: NodeJS.Platform, pathExt: string | undefined): string[] {
  const names = platform === 'win32'
    ? (pathExt ?? process.env.PATHEXT ?? '.EXE;.CMD;.BAT;.COM').split(';').filter(Boolean).map((ext) => `pnpm${ext}`)
    : ['pnpm']
  return pathEnv.split(platform === 'win32' ? ';' : ':').flatMap((dir) => {
    const base = dir === '' ? '.' : dir
    const spellings = platform === 'win32' ? spellingsOnDisk(base) : undefined
    return names
      .map((name) => path.join(base, spellings?.get(name.toLowerCase()) ?? name))
      .filter((candidate) => isExecutableFile(candidate, platform))
  })
}

/**
 * The names in `dir` keyed by their lower case form. Windows matches
 * `pnpm.CMD` from PATHEXT to `pnpm.cmd` on disk, and the warning should name
 * the file as it is spelled there.
 */
function spellingsOnDisk (dir: string): Map<string, string> {
  try {
    return new Map(fs.readdirSync(dir).map((entry) => [entry.toLowerCase(), entry]))
  } catch {
    return new Map()
  }
}

function isExecutableFile (file: string, platform: NodeJS.Platform): boolean {
  try {
    if (!fs.statSync(file).isFile()) return false
    if (platform !== 'win32') fs.accessSync(file, fs.constants.X_OK)
    return true
  } catch {
    return false
  }
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

function isOwnPnpm (executable: string, pnpmHome: string): boolean {
  const ownDirs = [path.join(pnpmHome, 'bin'), pnpmHome]
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
