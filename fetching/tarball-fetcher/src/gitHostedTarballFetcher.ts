import { type FetchFunction, type FetchOptions } from '@pnpm/fetcher-base'
import type { Cafs } from '@pnpm/cafs-types'
import { globalWarn } from '@pnpm/logger'
import { preparePackage } from '@pnpm/prepare-package'
import { addFilesFromDir } from '@pnpm/fetching.tarball-worker'

interface Resolution {
  integrity?: string
  registry?: string
  tarball: string
}

export interface CreateGitHostedTarballFetcher {
  ignoreScripts?: boolean
  rawConfig: object
  unsafePerm?: boolean
}

export function createGitHostedTarballFetcher (fetchRemoteTarball: FetchFunction, fetcherOpts: CreateGitHostedTarballFetcher): FetchFunction {
  const fetch = async (cafs: Cafs, resolution: Resolution, opts: FetchOptions) => {
    const { filesIndex } = await fetchRemoteTarball(cafs, resolution, opts)
    try {
      const prepareResult = await prepareGitHostedPkg(filesIndex as Record<string, string>, cafs, opts.filesIndexFile, fetcherOpts)
      if (prepareResult.ignoredBuild) {
        globalWarn(`The git-hosted package fetched from "${resolution.tarball}" has to be built but the build scripts were ignored.`)
      }
      return { filesIndex: prepareResult.filesIndex }
    } catch (err: any) { // eslint-disable-line
      err.message = `Failed to prepare git-hosted package fetched from "${resolution.tarball}": ${err.message}` // eslint-disable-line
      throw err
    }
  }

  return fetch as FetchFunction
}

async function prepareGitHostedPkg (filesIndex: Record<string, string>, cafs: Cafs, filesIndexFile: string, opts: CreateGitHostedTarballFetcher) {
  const tempLocation = await cafs.tempDir()
  cafs.importPackage(tempLocation, {
    filesResponse: {
      filesIndex,
      fromStore: false,
    },
    force: true,
  })
  const shouldBeBuilt = await preparePackage(opts, tempLocation)
  const newFilesIndex = await addFilesFromDir({
    cafsDir: cafs.cafsDir,
    dir: tempLocation,
    filesIndexFile,
  })
  // Important! We cannot remove the temp location at this stage.
  // Even though we have the index of the package,
  // the linking of files to the store is in progress.
  return {
    filesIndex: newFilesIndex,
    ignoredBuild: opts.ignoreScripts && shouldBeBuilt,
  }
}
