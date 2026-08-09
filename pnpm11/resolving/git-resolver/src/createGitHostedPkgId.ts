import type { PkgResolutionId } from '@pnpm/resolving.resolver-base'

import { normalizeGitRepoUrl } from './gitRepoUrl.js'

export function createGitHostedPkgId ({ repo, commit, path }: { repo: string, commit: string, path?: string }): PkgResolutionId {
  const normalizedRepo = normalizeGitRepoUrl(repo)
  let id = `${normalizedRepo.includes('://') ? '' : 'https://'}${normalizedRepo}#${commit}`
  if (!id.startsWith('git+')) id = `git+${id}`
  if (path) {
    id += `&path:${path}`
  }
  return id as PkgResolutionId
}
