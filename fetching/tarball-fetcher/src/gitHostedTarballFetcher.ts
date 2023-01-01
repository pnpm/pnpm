import { FetchFunction, FetchOptions } from '@pnpm/fetcher-base'
import type { Cafs, FilesIndex, PackageFileInfo } from '@pnpm/cafs-types'
import { preparePackage } from '@pnpm/prepare-package'
import pMapValues from 'p-map-values'
import omit from 'ramda/src/omit'

interface Resolution {
  integrity?: string
  registry?: string
  tarball: string
}

export interface CreateGitHostedTarballFetcher {
  rawConfig: object
  unsafePerm?: boolean
}

export function createGitHostedTarballFetcher (fetchRemoteTarball: FetchFunction, fetcherOpts: CreateGitHostedTarballFetcher): FetchFunction {
  const fetch = async (cafs: Cafs, resolution: Resolution, opts: FetchOptions) => {
    const { filesIndex } = await fetchRemoteTarball(cafs, resolution, opts)
    try {
      return { filesIndex: await prepareGitHostedPkg(filesIndex as FilesIndex, cafs, fetcherOpts) }
    } catch (err: any) { // eslint-disable-line
      err.message = `Failed to prepare git-hosted package fetched from "${resolution.tarball}": ${err.message}` // eslint-disable-line
      throw err
    }
  }

  return fetch as FetchFunction
}

async function prepareGitHostedPkg (filesIndex: FilesIndex, cafs: Cafs, opts: CreateGitHostedTarballFetcher) {
  const tempLocation = await cafs.tempDir()
  await cafs.importPackage(tempLocation, {
    filesResponse: {
      filesIndex: await waitForFilesIndex(filesIndex),
      fromStore: false,
    },
    force: true,
  })
  await preparePackage(opts, tempLocation)
  const newFilesIndex = await cafs.addFilesFromDir(tempLocation)
  // Important! We cannot remove the temp location at this stage.
  // Even though we have the index of the package,
  // the linking of files to the store is in progress.
  return newFilesIndex
}

export async function waitForFilesIndex (filesIndex: FilesIndex): Promise<Record<string, PackageFileInfo>> {
  return pMapValues(async (fileInfo) => {
    const { integrity, checkedAt } = await fileInfo.writeResult
    return {
      ...omit(['writeResult'], fileInfo),
      checkedAt,
      integrity: integrity.toString(),
    }
  }, filesIndex)
}
