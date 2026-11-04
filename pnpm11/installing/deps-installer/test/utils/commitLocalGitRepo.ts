import { pathToFileURL } from 'node:url'

import { safeExeca as execa } from 'execa'

/**
 * Commits everything in `repoDir` as the first commit of a new repository.
 *
 * @returns a `git+file:` URL pinned to that commit.
 */
export async function commitLocalGitRepo (repoDir: string): Promise<string> {
  await execa('git', ['init', '-q', '-b', 'main'], { cwd: repoDir })
  await execa('git', ['config', 'user.email', 'test@example.invalid'], { cwd: repoDir })
  await execa('git', ['config', 'user.name', 'Test'], { cwd: repoDir })
  await execa('git', ['add', '-A'], { cwd: repoDir })
  await execa('git', ['-c', 'commit.gpgsign=false', 'commit', '-q', '-m', 'init'], { cwd: repoDir })
  const commit = String((await execa('git', ['rev-parse', 'HEAD'], { cwd: repoDir })).stdout).trim()
  return `git+${pathToFileURL(repoDir).href}#${commit}`
}
