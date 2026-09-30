import fs from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'

import { nonInteractiveGitEnv } from '@pnpm/network.git-utils'
import { safeExeca as execa } from 'execa'

/**
 * `git ls-remote` lists tags without dates, so the tags are fetched shallowly
 * and without trees into a scratch repository, where each one's creation
 * date can be read: the tagger date of an annotated tag, the committer date of
 * a lightweight one.
 *
 * Whoever creates a tag or commit sets these dates, and the server does not
 * check them, so a backdated tag passes. Unlike a registry's publish time,
 * they hold back only releases that carry their real date.
 */
export async function readTagDates (repoUrl: string, tags: string[]): Promise<Record<string, Date>> {
  const scratch = await fs.mkdtemp(path.join(os.tmpdir(), 'pnpm-github-actions-'))
  try {
    const env = await nonInteractiveGitEnv()
    await execa('git', ['init', '--quiet', '--bare'], { cwd: scratch, env })
    const input = tags.map((tag) => `refs/tags/${tag}:refs/tags/${tag}\n`).join('')
    const fetchArgs = ['fetch', '--quiet', '--depth=1', '--filter=tree:0', '--no-tags', '--no-write-fetch-head', '--stdin', repoUrl]
    try {
      await execa('git', fetchArgs, { cwd: scratch, env, input })
    } catch {
      // One retry, like `git ls-remote`.
      await execa('git', fetchArgs, { cwd: scratch, env, input })
    }
    const { stdout } = await execa('git', ['for-each-ref', '--format=%(creatordate:unix) %(refname:strip=2)', 'refs/tags'], { cwd: scratch, env })
    return parseTagDates(stdout as string)
  } finally {
    await fs.rm(scratch, { force: true, recursive: true })
  }
}

function parseTagDates (forEachRefOutput: string): Record<string, Date> {
  const dates: Record<string, Date> = {}
  for (const line of forEachRefOutput.split('\n')) {
    const separator = line.indexOf(' ')
    const seconds = Number(line.slice(0, separator))
    if (separator > 0 && Number.isInteger(seconds)) dates[line.slice(separator + 1)] = new Date(seconds * 1000)
  }
  return dates
}
