import { spawnSync } from 'node:child_process'
import { appendFileSync } from 'node:fs'
import { pathToFileURL } from 'node:url'

export function determineTestScope ({ event, base, cwd = process.cwd() }) {
  const full = { script: 'ci:test-all', scope: 'all', full_tests: 'true', benchmark: 'tests.all' }
  if (event !== 'pull_request' || !/^(?:[a-f0-9]{40}|[a-f0-9]{64})$/.test(base ?? '')) return full
  const git = (...args) => {
    const result = spawnSync('git', args, { cwd, encoding: 'utf8' })
    if (result.error || result.status !== 0) throw new Error(result.stderr || 'git failed')
    return result.stdout
  }
  try {
    // Both package runners use this ref. Pin it to the PR event, not a newer main.
    git('fetch', '--no-tags', '--depth=1', 'origin', base)
    git('update-ref', 'refs/remotes/origin/main', base)
    const files = git('diff', '--no-renames', '--name-only', '-z', base, 'HEAD').split('\0').filter(Boolean)
    // Release notes cannot change tests; other root inputs can affect every package.
    if (files.some(file => !file.startsWith('pnpm11/') && !/^\.changeset\/[^/]+\.md$/.test(file))) return full
    return { script: 'ci:test-branch', scope: 'affected packages', full_tests: 'false', benchmark: 'tests.affected' }
  } catch (error) {
    console.warn(`Cannot establish affected test scope; running all tests: ${error.message}`)
    return full
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const scope = determineTestScope({ event: process.env.EVENT_NAME, base: process.env.PR_BASE_SHA })
  appendFileSync(process.env.GITHUB_OUTPUT, Object.entries(scope).map(([key, value]) => `${key}=${value}\n`).join(''))
}
