import fetchFromGit from '@pnpm/git-fetcher'
import createTarballFetcher from '@pnpm/tarball-fetcher'
import {PnpmOptions} from '@pnpm/types'
import {FetchFunction, FetchOptions, Resolution} from 'package-store'
import * as unpackStream from 'unpack-stream'

export default function (opts: PnpmOptions & {alwaysAuth: boolean, registry: string, strictSsl: boolean, rawNpmConfig: object}) {
  return fetcher.bind(null, {
    ...createTarballFetcher(opts),
    git: fetchFromGit,
  })
}

async function fetcher (
  fetcherByHostingType: {[hostingType: string]: FetchFunction},
  resolution: Resolution,
  target: string,
  opts: FetchOptions,
): Promise<unpackStream.Index> {
  const fetch = fetcherByHostingType[resolution.type || 'tarball']
  if (!fetch) {
    throw new Error(`Fetching for dependency type "${resolution.type}" is not supported`)
  }
  return await fetch(resolution, target, opts)
}
