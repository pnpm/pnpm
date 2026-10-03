import { PnpmError } from '@pnpm/error'
import { safeExeca as execa } from 'execa'

export interface GitVersion {
  major: number
  minor: number
}

/**
 * The oldest git the `[<since>]` selector runs on: `git diff` and
 * `git merge-base` need `--end-of-options`.
 */
const MIN_GIT_VERSION: GitVersion = { major: 2, minor: 24 }

let installedGitVersion: Promise<GitVersion | undefined> | undefined

/** The installed git's version, read once per process. */
export async function getGitVersion (): Promise<GitVersion | undefined> {
  installedGitVersion ??= readGitVersion()
  return installedGitVersion
}

async function readGitVersion (): Promise<GitVersion | undefined> {
  try {
    return parseGitVersion(String((await execa('git', ['--version'])).stdout))
  } catch {
    return undefined
  }
}

/**
 * Throws `ERR_PNPM_FILTER_CHANGED` for a git older than the minimum. An
 * unknown version passes, so a failing git reports its own error.
 */
export function checkGitVersion (version: GitVersion | undefined): void {
  if (version != null && !isAtLeast(version, MIN_GIT_VERSION)) {
    throw new PnpmError('FILTER_CHANGED', `Filtering by changed packages failed. The [<since>] selector requires git ${MIN_GIT_VERSION.major}.${MIN_GIT_VERSION.minor} or newer, but git ${version.major}.${version.minor} is installed.`)
  }
}

/**
 * Whether git accepts `git diff --no-relative`, which keeps a
 * `diff.relative=true` setting from printing paths relative to the workspace
 * directory. git added both the flag and the setting in 2.28, so an older git
 * rejects the flag and already prints paths from the repository root. An
 * unknown version counts as supported.
 */
export function gitSupportsNoRelative (version: GitVersion | undefined): boolean {
  return version == null || isAtLeast(version, { major: 2, minor: 28 })
}

function isAtLeast (version: GitVersion, minimum: GitVersion): boolean {
  return version.major > minimum.major || (version.major === minimum.major && version.minor >= minimum.minor)
}

/**
 * The major and minor version from `git --version` output, such as
 * `git version 2.39.3 (Apple Git-145)` or `git version 2.45.1.windows.1`.
 */
export function parseGitVersion (output: string): GitVersion | undefined {
  const prefix = 'git version '
  const trimmed = output.trim()
  if (!trimmed.startsWith(prefix)) return undefined
  const [major, minor] = trimmed.slice(prefix.length).split('.').map((part) => Number.parseInt(part, 10))
  if (major == null || minor == null || Number.isNaN(major) || Number.isNaN(minor)) return undefined
  return { major, minor }
}
