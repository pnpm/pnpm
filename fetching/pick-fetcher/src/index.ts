import type { Resolution } from '@pnpm/resolver-base'
import type { Fetchers, FetchFunction, DirectoryFetcher, GitFetcher, NodeRuntimeFetcher } from '@pnpm/fetcher-base'

export function pickFetcher (fetcherByHostingType: Partial<Fetchers>, resolution: Resolution): FetchFunction | DirectoryFetcher | GitFetcher | NodeRuntimeFetcher {
  let fetcherType: keyof Fetchers | undefined = resolution.type
  console.log('==>', resolution)

  if (resolution.type == null) {
    if (resolution.tarball.startsWith('file:')) {
      fetcherType = 'localTarball'
    } else if (isGitHostedPkgUrl(resolution.tarball)) {
      fetcherType = 'gitHostedTarball'
    } else {
      fetcherType = 'remoteTarball'
    }
  }

  console.log('==>', fetcherType)
  const fetch = fetcherByHostingType[fetcherType!]

  if (!fetch) {
    throw new Error(`Fetching for dependency type "${resolution.type ?? 'undefined'}" is not supported`)
  }

  return fetch
}

export function isGitHostedPkgUrl (url: string): boolean {
  return (
    url.startsWith('https://codeload.github.com/') ||
    url.startsWith('https://bitbucket.org/') ||
    url.startsWith('https://gitlab.com/')
  ) && url.includes('tar.gz')
}
