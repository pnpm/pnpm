const KNOWN_HOSTS = new Set(['github.com', 'gitlab.com', 'bitbucket.org'])

/**
 * `true` when a lockfile specifier and a manifest specifier denote the same
 * dependency. Falls back to Git specifier equivalence when they aren't byte
 * equal, so a lockfile written with the canonical `git+https://….git#<sha>`
 * form still satisfies a manifest that uses `git://`, a hosted shortcut, or
 * the bare `owner/repo` form. `undefined` on either side is never equal to a
 * present specifier.
 */
export function dependencySpecifiersAreEqual (
  lockfileSpecifier: string | undefined,
  manifestSpecifier: string | undefined
): boolean {
  if (lockfileSpecifier === manifestSpecifier) return true
  if (lockfileSpecifier == null || manifestSpecifier == null) return false
  return gitSpecifiersAreEquivalent(lockfileSpecifier, manifestSpecifier)
}

/**
 * Returns whether both inputs identify the same supported Git repository and
 * ref. Hosted shortcuts and bare GitHub shortcuts compare with `git://`,
 * `git+https://`, and hosted `https://` forms after adding or removing the
 * conventional `.git` suffix. Authentication-bearing, query-bearing, SSH, and
 * plain HTTP specifiers remain distinct.
 */
export function gitSpecifiersAreEquivalent (left: string, right: string): boolean {
  const normalizedLeft = normalizeGitSpecifier(left)
  if (normalizedLeft == null) return false
  const normalizedRight = normalizeGitSpecifier(right)
  if (normalizedRight == null) return false
  return normalizedLeft === normalizedRight
}

function normalizeGitSpecifier (specifier: string): string | undefined {
  return normalizeShortcut(specifier) ?? normalizeUrl(specifier)
}

function normalizeShortcut (specifier: string): string | undefined {
  const split = splitCommittish(specifier)
  if (split == null) return undefined
  const { repository, committish } = split
  let host: string
  let path: string
  if (repository.startsWith('github:')) {
    host = 'github.com'
    path = repository.slice('github:'.length)
  } else if (repository.startsWith('gitlab:')) {
    host = 'gitlab.com'
    path = repository.slice('gitlab:'.length)
  } else if (repository.startsWith('bitbucket:')) {
    host = 'bitbucket.org'
    path = repository.slice('bitbucket:'.length)
  } else if (isGithubShorthand(repository)) {
    host = 'github.com'
    path = repository
  } else {
    return undefined
  }
  return normalizeParts(host, path, committish)
}

function normalizeUrl (specifier: string): string | undefined {
  const split = splitCommittish(specifier)
  if (split == null) return undefined
  const { repository, committish } = split
  const schemeEnd = repository.indexOf('://')
  if (schemeEnd === -1) return undefined
  const scheme = repository.slice(0, schemeEnd).toLowerCase()
  if (scheme !== 'git' && scheme !== 'git+https' && scheme !== 'https') return undefined
  const location = repository.slice(schemeEnd + '://'.length)
  const slash = location.indexOf('/')
  if (slash === -1) return undefined
  const host = location.slice(0, slash)
  const path = location.slice(slash + 1)
  if (host === '' || host.includes('@') || host.includes('?') || /\s/.test(host)) return undefined
  if (scheme === 'https' && !KNOWN_HOSTS.has(host) && !path.endsWith('.git')) return undefined
  return normalizeParts(host, path, committish)
}

function normalizeParts (host: string, path: string, committish: string | undefined): string | undefined {
  if (
    path === '' ||
    path.startsWith('/') ||
    path.endsWith('/') ||
    path.includes('//') ||
    path.includes('@') ||
    path.includes('?') ||
    /\s/.test(path) ||
    path.split('/').some((segment) => segment === '')
  ) {
    return undefined
  }
  const repositoryPath = path.endsWith('.git') ? path.slice(0, -'.git'.length) : path
  if (repositoryPath === '' || repositoryPath.endsWith('/')) return undefined
  return committish != null && committish !== ''
    ? `git+https://${host}/${repositoryPath}.git#${committish}`
    : `git+https://${host}/${repositoryPath}.git`
}

function splitCommittish (specifier: string): { repository: string, committish: string | undefined } | undefined {
  const hashIndex = specifier.indexOf('#')
  if (hashIndex === -1) return { repository: specifier, committish: undefined }
  const committish = specifier.slice(hashIndex + 1)
  if (committish.includes('#')) return undefined
  return { repository: specifier.slice(0, hashIndex), committish }
}

function isGithubShorthand (repository: string): boolean {
  return !repository.startsWith('.') &&
    !/\s/.test(repository) &&
    !repository.includes(':') &&
    !repository.includes('@') &&
    repository.split('/').length === 2
}
