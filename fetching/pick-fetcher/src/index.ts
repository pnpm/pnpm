import type { Resolution } from '@pnpm/resolver-base'
import type { Fetchers, FetchFunction, DirectoryFetcher, GitFetcher } from '@pnpm/fetcher-base'

export function pickFetcher (fetcherByHostingType: Partial<Fetchers>, resolution: Resolution): FetchFunction | DirectoryFetcher | GitFetcher {
  let fetcherType: keyof Fetchers | undefined = resolution.type

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

// https://github.com/stackblitz-labs/pkg.pr.new
// With pkg.pr.new, each of your commits and pull requests will trigger an instant preview release without publishing anything to NPM.
// This enables users to access features and bug-fixes without the need to wait for release cycles using npm or pull request merges.
// When a package is installed via pkg.pr.new and has never been published to npm,
// the version or name obtained is incorrect, and an error will occur when patching. We can treat it as a tarball url.
export function isPkgPrNewUrl (url: string): boolean {
  return url.startsWith('https://pkg.pr.new/')
}

export function isGitHostedPkgUrl (url: string): boolean {
  return (
    url.startsWith('https://codeload.github.com/') ||
    url.startsWith('https://bitbucket.org/') ||
    url.startsWith('https://gitlab.com/')
  ) && url.includes('tar.gz')
}
