import assert from 'node:assert'

import { isError } from '@pnpm/error'
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
      assert(isError(err))
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
  const { shouldBeBuilt, pkgDir, ignoredBuild } = await unpackAndPreparePackage(
    cafs,
    filesMap,
    opts,
    fetcherOpts,
    resolution
  )

  filesIndexFile = resolveGitHostedIndexFile({
    filesIndexFile,
    ignoredBuild,
    ignoreScripts: opts.ignoreScripts,
    pkgResolutionId: fetcherOpts.pkgResolutionId ?? createGitHostedTarballPkgResolutionId(resolution),
    shouldBeBuilt,
  })

  const files = await packlist(pkgDir)
  const isUnchanged = !resolution.path && files.length === filesMap.size && (!shouldBeBuilt || ignoredBuild)
  if (isUnchanged) {
    return handleUnchangedGitPkg({
      filesIndexFile,
      filesMap,
      ignoredBuild,
      ignoreScripts: opts.ignoreScripts,
      rawFilesIndexFile,
      shouldBeBuilt,
      storeIndex: opts.storeIndex,
    })
  }

  return reindexAndAddGitPkgFiles({
    cafs,
    fetcherOpts,
    files,
    filesIndexFile,
    ignoredBuild,
    opts,
    pkgDir,
    rawFilesIndexFile,
    shouldBeBuilt,
  })
}

async function reindexAndAddGitPkgFiles (opts: {
  cafs: Cafs
  fetcherOpts: FetchOptions
  files: string[]
  filesIndexFile: string
  ignoredBuild: boolean
  opts: CreateGitHostedTarballFetcher
  pkgDir: string
  rawFilesIndexFile: string
  shouldBeBuilt: boolean
}): Promise<PrepareGitHostedPkgResult> {
  opts.opts.storeIndex.delete(opts.rawFilesIndexFile)
  return {
    filesIndexFile: opts.filesIndexFile,
    ...await addFilesFromDir({
      storeDir: opts.cafs.storeDir,
      storeIndex: opts.opts.storeIndex,
      dir: opts.pkgDir,
      files: opts.files,
      filesIndexFile: opts.filesIndexFile,
      pkg: opts.fetcherOpts.pkg,
      readManifest: opts.fetcherOpts.readManifest,
      requiresPrepare: opts.shouldBeBuilt,
    }),
    ignoredBuild: opts.ignoredBuild,
    requiresPrepare: opts.shouldBeBuilt,
  }
}

async function unpackAndPreparePackage (
  cafs: Cafs,
  filesMap: FilesMap,
  opts: CreateGitHostedTarballFetcher,
  fetcherOpts: FetchOptions,
  resolution: Resolution
): Promise<{ shouldBeBuilt: boolean, pkgDir: string, ignoredBuild: boolean }> {
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
  return { shouldBeBuilt, pkgDir, ignoredBuild }
}


function resolveGitHostedIndexFile (opts: {
  filesIndexFile: string
  ignoredBuild: boolean
  ignoreScripts?: boolean
  pkgResolutionId: string
  shouldBeBuilt: boolean
}): string {
  const shouldReindex = opts.shouldBeBuilt &&
    ((opts.ignoredBuild && !opts.ignoreScripts) || (!opts.ignoredBuild && opts.filesIndexFile.endsWith('\tnot-built')))
  if (shouldReindex) {
    return gitHostedStoreIndexKey(opts.pkgResolutionId, { built: !opts.ignoredBuild })
  }
  return opts.filesIndexFile
}

function handleUnchangedGitPkg (opts: {
  filesIndexFile: string
  filesMap: FilesMap
  ignoredBuild: boolean
  ignoreScripts?: boolean
  rawFilesIndexFile: string
  shouldBeBuilt: boolean
  storeIndex: StoreIndex
}): PrepareGitHostedPkgResult {
  if (!opts.ignoredBuild || !opts.ignoreScripts) {
    const data = opts.storeIndex.get(opts.rawFilesIndexFile) as { requiresPrepare?: boolean } | undefined
    if (data) {
      data.requiresPrepare = opts.shouldBeBuilt
      opts.storeIndex.set(opts.filesIndexFile, data)
    }
  }
  opts.storeIndex.delete(opts.rawFilesIndexFile)
  return {
    filesIndexFile: opts.filesIndexFile,
    filesMap: opts.filesMap,
    ignoredBuild: opts.ignoredBuild,
    requiresPrepare: opts.shouldBeBuilt,
  }
}

function createGitHostedTarballPkgResolutionId (resolution: Resolution): string {
  let pkgResolutionId = resolution.tarball
  if (resolution.path) {
    pkgResolutionId += `#path:${resolution.path}`
  }
  return pkgResolutionId
}
