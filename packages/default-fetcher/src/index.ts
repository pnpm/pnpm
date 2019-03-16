import fetchFromGit from '@pnpm/git-fetcher'
import {
  DirectoryResolution,
  TarballResolution,
} from '@pnpm/resolver-base'
import createTarballFetcher, {
  IgnoreFunction,
} from '@pnpm/tarball-fetcher'

export default function (
  opts: {
    alwaysAuth?: boolean,
    fsIsCaseSensitive?: boolean,
    registry: string,
    rawNpmConfig: object,
    strictSsl?: boolean,
    proxy?: string,
    httpsProxy?: string,
    localAddress?: string,
    cert?: string,
    key?: string,
    ca?: string,
    fetchRetries?: number,
    fetchRetryFactor?: number,
    fetchRetryMintimeout?: number,
    fetchRetryMaxtimeout?: number,
    userAgent?: string,
    ignoreFile?: IgnoreFunction,
    offline?: boolean,
  },
) {
  return {
    ...createTarballFetcher(opts),
    ...fetchFromGit(),
  }
}
