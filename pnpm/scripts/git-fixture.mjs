import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import process from 'node:process'

// A committer the machine running the tests does not have to be configured for.
const IDENTITY = {
  GIT_AUTHOR_NAME: 't',
  GIT_AUTHOR_EMAIL: 't@t',
  GIT_COMMITTER_NAME: 't',
  GIT_COMMITTER_EMAIL: 't@t',
}

/**
 * Runs a git command in `repo`, failing the test when it does not succeed.
 *
 * A fixture command that fails silently leaves a test asserting against a
 * repository that was never set up.
 */
export function git (repo, ...args) {
  const result = spawnSync('git', args, { cwd: repo, encoding: 'utf8', env: { ...process.env, ...IDENTITY } })
  assert.equal(result.status, 0, `git ${args.join(' ')} failed: ${result.stderr}`)
  return result.stdout
}

/** An empty repository, removed when the test ends. */
export function temporaryRepo (context, prefix) {
  // The real path, so that a test comparing the paths a script reports against
  // its own does not trip over a symlinked temporary directory.
  const repo = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), prefix)))
  context.after(() => fs.rmSync(repo, { recursive: true, force: true }))
  git(repo, 'init')
  return repo
}
