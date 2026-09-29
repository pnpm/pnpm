import { execFileSync } from 'node:child_process'
import { appendFileSync, readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

export function prepareDocsSync ({ event, eventName, releaseTag, docsRef: correction, git = runGit, publicationState = readPublicationState }) {
  const automatic = eventName === 'workflow_run'
  const tag = automatic ? event.workflow_run.head_branch : releaseTag
  const match = /^(v|pnpr@)(\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?)$/.exec(tag ?? '')
  if (!match) {
    if (automatic) return undefined
    throw new Error('Expected a pnpm or pnpr release tag such as v12.8.2 or pnpr@0.1.0-alpha.15')
  }
  const [, prefix, version] = match
  if (prefix === 'v' && !['11', '12'].includes(version.split('.')[0])) {
    if (automatic) return undefined
    throw new Error('Only pnpm v11 and v12 documentation is maintained here')
  }
  if (prefix === 'v' && version.includes('-')) return undefined
  const releaseCommit = git('rev-parse', '--verify', `refs/tags/${tag}^{commit}`)
  if (automatic && releaseCommit !== event.workflow_run.head_sha) throw new Error('Release tag does not match the completed workflow')
  git('verify-tag', tag)
  const manifests = prefix === 'pnpr@' ? ['pnpr/npm/pnpr/package.json'] : ['pnpm/npm/pnpm/package.json', 'pnpm11/pnpm/package.json']
  if (!manifests.some(file => JSON.parse(git('show', `${releaseCommit}:${file}`)).version === version)) {
    throw new Error(`Tag ${tag} does not match a committed product version`)
  }
  const packageName = prefix === 'pnpr@' ? '@pnpm/pnpr' : 'pnpm'
  const publication = publicationState(`${packageName}@${version}`)
  if (publication !== 'published') throw new Error(`${packageName}@${version} has not been published`)
  const line = prefix === 'pnpr@' ? 'pnpr' : `${version.split('.')[0]}.x`
  const docsRef = automatic ? releaseCommit : correction || releaseCommit
  if (!/^[a-f0-9]{40}$/.test(docsRef)) throw new Error('Documentation corrections require a full commit SHA')
  const docsCommit = git('rev-parse', '--verify', `${docsRef}^{commit}`)
  if (docsCommit !== releaseCommit) {
    git('merge-base', '--is-ancestor', releaseCommit, docsCommit)
    const docsPath = line === 'pnpr' ? 'pnpr/docs' : line === '11.x' ? 'pnpm11/docs' : 'pnpm/docs'
    const changed = git('diff', '--name-only', releaseCommit, docsCommit).split('\n').filter(Boolean)
    if (!changed.length || changed.some(file => !file.startsWith(`${docsPath}/`))) {
      throw new Error(`Corrections must be based on ${tag} and change only ${docsPath}/`)
    }
  }
  return { publish: true, line, version, release_commit: releaseCommit, docs_commit: docsCommit }
}

function readPublicationState (spec) {
  return execFileSync('bash', ['.github/scripts/npm-package-publication-state.sh', spec], { encoding: 'utf8' }).trim()
}

function runGit (...args) {
  return execFileSync('git', args, { encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] }).trim()
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const result = prepareDocsSync({
    event: JSON.parse(readFileSync(process.env.GITHUB_EVENT_PATH, 'utf8')),
    eventName: process.env.GITHUB_EVENT_NAME,
    releaseTag: process.env.RELEASE_TAG,
    docsRef: process.env.DOCS_COMMIT,
  })
  if (result) appendFileSync(process.env.GITHUB_OUTPUT, Object.entries(result).map(([key, value]) => `${key}=${value}\n`).join(''))
}
