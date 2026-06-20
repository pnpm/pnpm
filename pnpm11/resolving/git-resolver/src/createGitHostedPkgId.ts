import type { PkgResolutionId } from '@pnpm/resolving.resolver-base'

export function createGitHostedPkgId ({ repo, commit, path }: { repo: string, commit: string, path?: string }): PkgResolutionId {
  const normalizedRepo = normalizeGitRepoForPkgResolutionId(repo)
  let id = `${normalizedRepo.includes('://') ? '' : 'https://'}${normalizedRepo}#${commit}`
  if (!id.startsWith('git+')) id = `git+${id}`
  if (path) {
    id += `&path:${path}`
  }
  return id as PkgResolutionId
}

function normalizeGitRepoForPkgResolutionId (repo: string): string {
  // Only scp-style shorthand (`user@host:path`) needs rewriting. A repo that
  // already carries a URL scheme (e.g. `ssh://user@host:2222/path`) is left
  // alone — its `@host:port` would otherwise match the scp pattern and get
  // mangled into `ssh://ssh://…`.
  if (repo.includes('://')) return repo
  const scp = /^([^@\s]+@[^:\s]+):(.+)$/.exec(repo)
  return scp == null ? repo : `ssh://${scp[1]}/${scp[2]}`
}
