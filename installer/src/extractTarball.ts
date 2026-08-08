import { spawnSync } from 'node:child_process'

/**
 * Extracts `member` (a path inside the tarball's `package/` root) into `dest`,
 * dropping that root. Shells out to `tar`, which every supported host has:
 * macOS and Linux ship it, and Windows has had bsdtar since Windows 10 1803.
 */
export function extractTarballMember (tarball: string, dest: string, member: string): void {
  const { error, status, stderr } = spawnSync(
    'tar',
    ['-xzf', tarball, '-C', dest, '--strip-components=1', member],
    { encoding: 'utf8' }
  )
  if (error != null) {
    if ((error as NodeJS.ErrnoException).code === 'ENOENT') {
      throw new Error('This installer needs the `tar` command, which was not found on your PATH.')
    }
    throw error
  }
  if (status !== 0) {
    throw new Error(`Could not extract ${member} from ${tarball}: ${stderr.trim()}`)
  }
}
