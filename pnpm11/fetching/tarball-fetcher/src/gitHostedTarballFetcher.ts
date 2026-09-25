import assert from 'node:assert'
import util from 'node:util'

import { preparePackage } from '@pnpm/exec.prepare-package'
import type { FetchFunction, FetchOptions } from '@pnpm/fetching.fetcher-base'
import { packlist } from '@pnpm/fs.packlist'
import { globalWarn } from '@pnpm/logger'
import type { Cafs, FilesMap } from '@pnpm/store.cafs-types'
import { gitHostedStoreIndexKey, type StoreIndex } from '@pnpm/store.index'
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
  storeIndex: StoreIndex
  unsafePerm?: boolean
}

export function createGitHostedTarballFetcher (fetchRemoteTarball: FetchFunction, fetcherOpts: CreateGitHostedTarballFetcher): FetchFunction {
  const fetch = async (cafs: Cafs, resolution: Resolution, opts: FetchOptions) => {
    const rawFilesIndexFile = `${opts.filesIndexFile}\traw`
    const { filesMap, manifest, requiresBuild, integrity } = await fetchRemoteTarball(cafs, resolution, {
      ...opts,
      filesIndexFile: rawFilesIndexFile,
      // The raw archive is not the prepared package, so it must not be indexed under the package's integrity key.
      pkgResolutionId: undefined,
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
        filesIndexFile: prepareResult.filesIndexFile,
        filesMap: prepareResult.filesMap,
        manifest: prepareResult.manifest ?? manifest,
        requiresBuild,
        requiresPrepare: prepareResult.requiresPrepare,
        ignoredBuild: prepareResult.ignoredBuild,
        // Propagate the raw tarball integrity so the lockfile pins it and
        // future installs detect a tampered tarball from the git host.
        integrity,
      }
    } catch (err: unknown) {
      assert(util.types.isNativeError(err))
      err.message = `Failed to prepare git-hosted package fetched from "${resolution.tarball}": ${err.message}`
      throw err
    }
  }

  return Object.assign(fetch, {
    resolutionNeedsFetch: fetchRemoteTarball.resolutionNeedsFetch?.bind(fetchRemoteTarball),
  }) as FetchFunction
}

interface PrepareGitHostedPkgResult {
  filesIndexFile: string
  filesMap: FilesMap
  manifest?: BundledManifest
  ignoredBuild: boolean
  requiresPrepare: boolean
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
  const { shouldBeBuilt, pkgDir, ignoredBuild = false } = await preparePackage({
    ...opts,
    allowBuild: fetcherOpts.allowBuild,
    pkgResolutionId: fetcherOpts.pkgResolutionId ?? createGitHostedTarballPkgResolutionId(resolution),
  }, tempLocation, resolution.path ?? '')
  if (shouldBeBuilt && ((ignoredBuild && !opts.ignoreScripts) || (!ignoredBuild && filesIndexFile.endsWith('\tnot-built')))) {
    filesIndexFile = gitHostedStoreIndexKey(fetcherOpts.pkgResolutionId ?? createGitHostedTarballPkgResolutionId(resolution), { built: !ignoredBuild })
  }
  const files = await packlist(pkgDir)
  const { storeIndex } = opts
  if (!resolution.path && files.length === filesMap.size) {
    if (!shouldBeBuilt || ignoredBuild) {
      if (!ignoredBuild || !opts.ignoreScripts) {
        const data = storeIndex.get(rawFilesIndexFile) as { requiresPrepare?: boolean } | undefined
        if (data) {
          data.requiresPrepare = shouldBeBuilt
          storeIndex.set(filesIndexFile, data)
        }
      }
      storeIndex.delete(rawFilesIndexFile)
      return {
        filesIndexFile,
        filesMap,
        ignoredBuild,
        requiresPrepare: shouldBeBuilt,
      }
    }
  }
  storeIndex.delete(rawFilesIndexFile)
  // Important! We cannot remove the temp location at this stage.
  // Even though we have the index of the package,
  // the linking of files to the store is in progress.
  return {
    filesIndexFile,
    ...await addFilesFromDir({
      storeDir: cafs.storeDir,
      storeIndex: opts.storeIndex,
      dir: pkgDir,
      files,
      filesIndexFile,
      pkg: fetcherOpts.pkg,
      readManifest: fetcherOpts.readManifest,
      requiresPrepare: shouldBeBuilt,
    }),
    ignoredBuild,
    requiresPrepare: shouldBeBuilt,
  }
}

function createGitHostedTarballPkgResolutionId (resolution: Resolution): string {
  let pkgResolutionId = resolution.tarball
  if (resolution.path) {
    pkgResolutionId += `#path:${resolution.path}`
  }
  return pkgResolutionId
}
