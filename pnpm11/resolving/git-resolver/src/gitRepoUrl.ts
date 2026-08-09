import { URL } from 'node:url'

/**
 * The URL form of a git repository reference, giving scp-style shorthand
 * (`user@host:path`) the `ssh://` scheme it implies.
 *
 * A reference that already carries a scheme (e.g. `ssh://user@host:2222/path`)
 * is left alone — its `@host:port` would otherwise match the scp pattern and
 * get mangled into `ssh://ssh://…`.
 */
export function normalizeGitRepoUrl (repo: string): string {
  if (repo.includes('://')) return repo
  const scp = /^([^@\s]+@[^:\s]+):(.+)$/.exec(repo)
  return scp == null ? repo : `ssh://${scp[1]}/${scp[2]}`
}

/**
 * The HTTPS equivalent of an SSH git repository reference, or `undefined` if
 * `repo` is not one.
 *
 * The SSH user and port are dropped: neither carries over to HTTPS, where the
 * host serves git over 443 and credentials come from git's credential helpers.
 */
export function sshRepoUrlToHttps (repo: string): string | undefined {
  const sshUrl = normalizeGitRepoUrl(repo).replace(/^git\+/, '')
  if (!sshUrl.startsWith('ssh://')) return undefined
  let parsed: URL
  try {
    parsed = new URL(sshUrl)
  } catch {
    return undefined
  }
  if (!parsed.hostname || parsed.pathname.length <= 1) return undefined
  return `https://${parsed.hostname}${parsed.pathname}`
}
