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
  const scp = /^([^@\s]+@[^:\s]+):(.+)$/.exec(repo)
  return scp == null ? repo : `ssh://${scp[1]}/${scp[2]}`
}
