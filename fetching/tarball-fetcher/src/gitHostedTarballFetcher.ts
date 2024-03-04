import assert from 'assert'
import fs from 'node:fs/promises'
import util from 'util'
import { type FetchFunction, type FetchOptions } from '@pnpm/fetcher-base'
import type { Cafs } from '@pnpm/cafs-types'
import { packlist } from '@pnpm/fs.packlist'
import { globalWarn } from '@pnpm/logger'
import { preparePackage } from '@pnpm/prepare-package'
import { type DependencyManifest } from '@pnpm/types'
import { addFilesFromDir } from '@pnpm/worker'
import renameOverwrite from 'rename-overwrite'
import { fastPathTemp as pathTemp } from 'path-temp'

interface Resolution {
  integrity?: string
  registry?: string
  tarball: string
  path?: string
}

export interface CreateGitHostedTarballFetcher {
  ignoreScripts?: boolean
  rawConfig: object
  unsafePerm?: boolean
}

export function createGitHostedTarballFetcher (fetchRemoteTarball: FetchFunction, fetcherOpts: CreateGitHostedTarballFetcher): FetchFunction {
  const fetch = async (cafs: Cafs, resolution: Resolution, opts: FetchOptions) => {
    const tempIndexFile = pathTemp(opts.filesIndexFile)
    const { filesIndex, manifest, requiresBuild } = await fetchRemoteTarball(cafs, resolution, {
      ...opts,
      filesIndexFile: tempIndexFile,
    })
    try {
      const prepareResult = await prepareGitHostedPkg(filesIndex as Record<string, string>, cafs, tempIndexFile, opts.filesIndexFile, fetcherOpts, opts, resolution)
      if (prepareResult.ignoredBuild) {
        globalWarn(`The git-hosted package fetched from "${resolution.tarball}" has to be built but the build scripts were ignored.`)
      }
      return {
        filesIndex: prepareResult.filesIndex,
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
  filesIndex: Record<string, string>
  manifest?: DependencyManifest
  ignoredBuild: boolean
}

async function prepareGitHostedPkg (
  filesIndex: Record<string, string>,
  cafs: Cafs,
  filesIndexFileNonBuilt: string,
  filesIndexFile: string,
  opts: CreateGitHostedTarballFetcher,
  fetcherOpts: FetchOptions,
  resolution: Resolution
): Promise<PrepareGitHostedPkgResult> {
  const tempLocation = await cafs.tempDir()
  cafs.importPackage(tempLocation, {
    filesResponse: {
      filesIndex,
      resolvedFrom: 'remote',
      requiresBuild: false,
    },
    force: true,
  })
  const { shouldBeBuilt, pkgDir } = await preparePackage(opts, tempLocation, resolution.path ?? '')
  const files = await packlist(pkgDir)
  if (!resolution.path && files.length === Object.keys(filesIndex).length) {
    if (!shouldBeBuilt) {
      if (filesIndexFileNonBuilt !== filesIndexFile) {
        await renameOverwrite(filesIndexFileNonBuilt, filesIndexFile)
      }
      return {
        filesIndex,
        ignoredBuild: false,
      }
    }
    if (opts.ignoreScripts) {
      return {
        filesIndex,
        ignoredBuild: true,
      }
    }
  }
  try {
    // The temporary index file may be deleted
    await fs.unlink(filesIndexFileNonBuilt)
  } catch {}
  // Important! We cannot remove the temp location at this stage.
  // Even though we have the index of the package,
  // the linking of files to the store is in progress.
  return {
    ...await addFilesFromDir({
      cafsDir: cafs.cafsDir,
      dir: pkgDir,
      files,
      filesIndexFile,
      pkg: fetcherOpts.pkg,
      readManifest: fetcherOpts.readManifest,
    }),
    ignoredBuild: Boolean(opts.ignoreScripts),
  }
}
