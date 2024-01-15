import { type FetchFunction, type FetchOptions } from '@pnpm/fetcher-base'
import type { Cafs } from '@pnpm/cafs-types'
import { globalWarn } from '@pnpm/logger'
import { preparePackage } from '@pnpm/prepare-package'
import { addFilesFromDir } from '@pnpm/worker'
import renameOverwrite from 'rename-overwrite'

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
    //(not sure) This should just extract the tarball to a temp directory. No need to fetch it to the store?!
    const nonBuiltIndexFile = opts.filesIndexFile + 'raw'
    const { filesIndex, manifest } = await fetchRemoteTarball(cafs, resolution, {
      ...opts,
      filesIndexFile: nonBuiltIndexFile
    })
    try {
      const prepareResult = await prepareGitHostedPkg(filesIndex as Record<string, string>, cafs, nonBuiltIndexFile, opts.filesIndexFile, fetcherOpts, opts)
      if (prepareResult.ignoredBuild) {
        globalWarn(`The git-hosted package fetched from "${resolution.tarball}" has to be built but the build scripts were ignored.`)
      }
      return { filesIndex: prepareResult.filesIndex, manifest }
    } catch (err: any) { // eslint-disable-line
      err.message = `Failed to prepare git-hosted package fetched from "${resolution.tarball}": ${err.message}`
      throw err
    }
  }

  return fetch as FetchFunction
}

async function prepareGitHostedPkg (
  filesIndex: Record<string, string>,
  cafs: Cafs,
  filesIndexFileNonBuilt: string,
  filesIndexFile: string,
  opts: CreateGitHostedTarballFetcher,
  fetcherOpts: FetchOptions
) {
  const tempLocation = await cafs.tempDir()
  cafs.importPackage(tempLocation, {
    filesResponse: {
      filesIndex,
      resolvedFrom: 'remote',
    },
    force: true,
  })
  const shouldBeBuilt = await preparePackage(opts, tempLocation)
  if (!shouldBeBuilt) {
    renameOverwrite(filesIndexFileNonBuilt, filesIndexFile)
    return {
      filesIndex,
      ignoredBuild: false,
    }
  }
  // Important! We cannot remove the temp location at this stage.
  // Even though we have the index of the package,
  // the linking of files to the store is in progress.
  return {
    ...await addFilesFromDir({
      cafsDir: cafs.cafsDir,
      dir: tempLocation,
      filesIndexFile,
      pkg: fetcherOpts.pkg,
    }),
    ignoredBuild: opts.ignoreScripts && shouldBeBuilt
  }
}
