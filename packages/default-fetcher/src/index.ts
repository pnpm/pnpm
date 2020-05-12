import fetchFromGit from '@pnpm/git-fetcher'
import createTarballFetcher from '@pnpm/tarball-fetcher'

export default function (
  opts: {
    alwaysAuth?: boolean,
    registry: string,
    rawConfig: object,
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
    offline?: boolean,
  }
) {
  return {
    ...createTarballFetcher(opts),
    ...fetchFromGit(),
  }
}
