import '@total-typescript/ts-reset'
import type { DirectoryFetcher, FetchFunction, FetchOptions, FetchResult, Fetchers, GitFetcher, Resolution } from '@pnpm/types'

export function pickFetcher(
  fetcherByHostingType: Partial<Fetchers>,
  resolution: Resolution
): DirectoryFetcher | GitFetcher | FetchFunction<Resolution, FetchOptions, FetchResult> {
  let fetcherType: string | undefined = resolution.type

  if (resolution.type == null) {
    if (resolution.tarball.startsWith('file:')) {
      fetcherType = 'localTarball'
    } else if (isGitHostedPkgUrl(resolution.tarball)) {
      fetcherType = 'gitHostedTarball'
    } else {
      fetcherType = 'remoteTarball'
    }
  }

  if (typeof fetcherType === 'undefined') {
    throw new Error(
      `Fetching for dependency type "${resolution.type ?? 'undefined'}" is not supported`
    )
  }

  const fetch = fetcherByHostingType[fetcherType as keyof Fetchers]

  if (!fetch) {
    throw new Error(
      `Fetching for dependency type "${resolution.type ?? 'undefined'}" is not supported`
    )
  }

  return fetch
}

export function isGitHostedPkgUrl(url: string) {
  return (
    (url.startsWith('https://codeload.github.com/') ||
      url.startsWith('https://bitbucket.org/') ||
      url.startsWith('https://gitlab.com/')) &&
    url.includes('tar.gz')
  )
}
