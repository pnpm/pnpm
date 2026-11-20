import path from 'node:path'

import type { FetchFunction, FetchOptions } from '@pnpm/fetching.fetcher-base'
import gfs from '@pnpm/fs.graceful-fs'
import { DEFAULT_MAX_TARBALL_UNPACKED_SIZE } from '@pnpm/store.cafs'
import type { Cafs } from '@pnpm/store.cafs-types'
import type { StoreIndex } from '@pnpm/store.index'
import { addFilesFromTarball } from '@pnpm/worker'

import { MAX_BUFFERED_DOWNLOAD_SIZE } from './remoteTarballFetcher.js'

const isAbsolutePath = /^\/|^[A-Z]:/i

interface Resolution {
  integrity?: string
  registry?: string
  tarball: string
}

export function createLocalTarballFetcher (storeIndex: StoreIndex, { maxTarballUnpackedSize = DEFAULT_MAX_TARBALL_UNPACKED_SIZE }: { maxTarballUnpackedSize?: number } = {}): FetchFunction {
  const fetch = async (cafs: Cafs, resolution: Resolution, opts: FetchOptions) => {
    const tarball = resolvePath(opts.lockfileDir, resolution.tarball.slice(5))
    const source = gfs.statSync(tarball).size > MAX_BUFFERED_DOWNLOAD_SIZE
      ? { tarballFile: tarball }
      : { buffer: gfs.readFileSync(tarball) }
    const result = await addFilesFromTarball({
      storeDir: cafs.storeDir,
      storeIndex,
      ...source,
      filesIndexFile: opts.filesIndexFile,
      integrity: resolution.integrity,
      readManifest: opts.readManifest,
      url: tarball,
      pkg: opts.pkg,
      appendManifest: opts.appendManifest,
      ignoreFilePattern: opts.ignoreFilePattern,
      maxTarballUnpackedSize,
    })
    return { ...result, local: true }
  }

  return fetch as FetchFunction
}

function resolvePath (where: string, spec: string): string {
  if (isAbsolutePath.test(spec)) return spec
  return path.resolve(where, spec)
}
