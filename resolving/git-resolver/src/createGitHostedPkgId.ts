import { type PkgResolutionId } from '@pnpm/resolver-base'

export function createGitHostedPkgId ({ repo, commit, path }: { repo: string, commit: string, path?: string }): PkgResolutionId {
  let id = `${repo.includes('://') ? '' : 'https://'}${repo}#${commit}`
  if (!id.startsWith('git+')) id = `git+${id}`
  if (path) {
    id += `&path:${path}`
  }
  return id as PkgResolutionId
}
