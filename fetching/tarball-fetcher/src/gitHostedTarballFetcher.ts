import assert from 'node:assert'
import util from 'node:util'

import type { Cafs, FilesMap } from '@pnpm/cafs-types'
import type { FetchFunction, FetchOptions } from '@pnpm/fetcher-base'
import { packlist } from '@pnpm/fs.packlist'
import { globalWarn } from '@pnpm/logger'
import { preparePackage } from '@pnpm/prepare-package'
import type { StoreIndex } from '@pnpm/store.index'
import type { BundledManifest } from '@pnpm/types'
import { addFilesFromDir } from '@pnpm/worker'

interface Resolution {
  integrity?: string
  registry?: string
  tarball: string
  path?: string
}

export interface CreateGitHostedTarballFetcher {
  ignoreScripts?: boolean
  rawConfig: Record<string, unknown>
  storeIndex: StoreIndex
  unsafePerm?: boolean
}

export function createGitHostedTarballFetcher (fetchRemoteTarball: FetchFunction, fetcherOpts: CreateGitHostedTarballFetcher): FetchFunction {
  const fetch = async (cafs: Cafs, resolution: Resolution, opts: FetchOptions) => {
    const rawFilesIndexFile = `${opts.filesIndexFile}\traw`
    const { filesMap, manifest, requiresBuild } = await fetchRemoteTarball(cafs, resolution, {
      ...opts,
      filesIndexFile: rawFilesIndexFile,
    })
    // Flush any queued store index writes so that the raw files index entry
    // written during tarball extraction is visible to subsequent reads.
    fetcherOpts.storeIndex.flush()
    try {
      const prepareResult = await prepareGitHostedPkg(filesMap, cafs, rawFilesIndexFile, opts.filesIndexFile, fetcherOpts, opts, resolution)
      if (prepareResult.ignoredBuild) {
        globalWarn(`The git-hosted package fetched from "${resolution.tarball}" has to be built but the build scripts were ignored.`)
      }
      return {
        filesMap: prepareResult.filesMap,
        manifest: prepareResult.manifest ?? manifest,
        requiresBuild,
      }
    } catch (err: unknown) {
      assert(util.types.isNativeError(err))
      err.message = `Failed to prepare git-hosted package fetched from "${resolution.tarball}": ${err.message}`
      throw err
    }
  }

  return fetch as FetchFunction
}

interface PrepareGitHostedPkgResult {
  filesMap: FilesMap
  manifest?: BundledManifest
  ignoredBuild: boolean
}

async function prepareGitHostedPkg (
  filesMap: FilesMap,
  cafs: Cafs,
  rawFilesIndexFile: string,
  filesIndexFile: string,
  opts: CreateGitHostedTarballFetcher,
  fetcherOpts: FetchOptions,
  resolution: Resolution
): Promise<PrepareGitHostedPkgResult> {
  const tempLocation = await cafs.tempDir()
  cafs.importPackage(tempLocation, {
    filesResponse: {
      filesMap,
      resolvedFrom: 'remote',
      requiresBuild: false,
    },
    force: true,
  })
  const { shouldBeBuilt, pkgDir } = await preparePackage({
    ...opts,
    allowBuild: fetcherOpts.allowBuild,
  }, tempLocation, resolution.path ?? '')
  const files = await packlist(pkgDir)
  const { storeIndex } = opts
  if (!resolution.path && files.length === filesMap.size) {
    if (!shouldBeBuilt) {
      const data = storeIndex.get(rawFilesIndexFile)
      if (data) {
        storeIndex.set(filesIndexFile, data)
        storeIndex.delete(rawFilesIndexFile)
      }
      return {
        filesMap,
        ignoredBuild: false,
      }
    }
    if (opts.ignoreScripts) {
      storeIndex.delete(rawFilesIndexFile)
      return {
        filesMap,
        ignoredBuild: true,
      }
    }
  }
  storeIndex.delete(rawFilesIndexFile)
  // Important! We cannot remove the temp location at this stage.
  // Even though we have the index of the package,
  // the linking of files to the store is in progress.
  return {
    ...await addFilesFromDir({
      storeDir: cafs.storeDir,
      storeIndex: opts.storeIndex,
      dir: pkgDir,
      files,
      filesIndexFile,
      pkg: fetcherOpts.pkg,
      readManifest: fetcherOpts.readManifest,
    }),
    ignoredBuild: Boolean(opts.ignoreScripts),
  }
}
