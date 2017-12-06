import fetchFromGit from '@pnpm/git-fetcher'
import createTarballFetcher from '@pnpm/tarball-fetcher'
import {PnpmOptions} from '@pnpm/types'

export default function (opts: PnpmOptions & {alwaysAuth: boolean, registry: string, strictSsl: boolean, rawNpmConfig: object}) {
  return {
    ...createTarballFetcher(opts),
    ...fetchFromGit(),
  }
}
