import { spawnSync } from 'node:child_process'
import fs from 'node:fs'

/**
 * Unpacks a package tarball into `dest`, leaving its `package/` root in place.
 *
 * Shells out to `tar`, which every supported host has: macOS and Linux ship it,
 * and Windows has had bsdtar since Windows 10 1803. Member selection and
 * `--strip-components` are avoided because busybox tar and bsdtar disagree
 * about them.
 */
export function extractTarball (tarball: string, dest: string): void {
  fs.mkdirSync(dest, { recursive: true })
  const { error, status, stderr } = spawnSync('tar', ['-xzf', tarball, '-C', dest], { encoding: 'utf8' })
  if (error != null) {
    if ((error as NodeJS.ErrnoException).code === 'ENOENT') {
      throw new Error('This installer needs the `tar` command, which was not found on your PATH.')
    }
    throw error
  }
  if (status !== 0) {
    throw new Error(`Could not extract ${tarball}: ${stderr.trim()}`)
  }
}
