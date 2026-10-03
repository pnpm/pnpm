import { safeExeca as execa } from 'execa'

let noRelativeSupport: Promise<boolean> | undefined

/**
 * Whether git accepts `git diff --no-relative`, which keeps a
 * `diff.relative=true` setting from printing paths relative to the workspace
 * directory. git added both the flag and the setting in 2.28, so an older git
 * rejects the flag and already prints paths from the repository root. An
 * unreadable `git --version` counts as supported.
 */
export async function gitSupportsNoRelative (): Promise<boolean> {
  noRelativeSupport ??= detectNoRelativeSupport()
  return noRelativeSupport
}

async function detectNoRelativeSupport (): Promise<boolean> {
  let output: string
  try {
    output = String((await execa('git', ['--version'])).stdout)
  } catch {
    return true
  }
  const version = parseGitVersion(output)
  return version == null || version.major > 2 || (version.major === 2 && version.minor >= 28)
}

/**
 * The major and minor version from `git --version` output, such as
 * `git version 2.39.3 (Apple Git-145)` or `git version 2.45.1.windows.1`.
 */
export function parseGitVersion (output: string): { major: number, minor: number } | undefined {
  const prefix = 'git version '
  const trimmed = output.trim()
  if (!trimmed.startsWith(prefix)) return undefined
  const [major, minor] = trimmed.slice(prefix.length).split('.').map((part) => Number.parseInt(part, 10))
  if (major == null || minor == null || Number.isNaN(major) || Number.isNaN(minor)) return undefined
  return { major, minor }
}
