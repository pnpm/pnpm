import type { Resolution } from '@pnpm/resolver-base'
import type { FetchFunction } from '@pnpm/fetcher-base'

export function pickFetcher (fetcherByHostingType: {[hostingType: string]: FetchFunction}, resolution: Resolution) {
  let fetcherType = resolution.type

  if (resolution.type == null) {
    if (resolution.tarball.startsWith('file:')) {
      fetcherType = 'localTarball'
    } else if (isGitHostedPkgUrl(resolution.tarball)) {
      fetcherType = 'gitHostedTarball'
    } else {
      fetcherType = 'remoteTarball'
    }
  }

  const fetch = fetcherByHostingType[fetcherType!]

  if (!fetch) {
    throw new Error(`Fetching for dependency type "${resolution.type ?? 'undefined'}" is not supported`)
  }

  return fetch
}

function isGitHostedPkgUrl (url: string) {
  return (
    url.startsWith('https://codeload.github.com/') ||
    url.startsWith('https://bitbucket.org/') ||
    url.startsWith('https://gitlab.com/')
  ) && url.includes('tar.gz')
}
