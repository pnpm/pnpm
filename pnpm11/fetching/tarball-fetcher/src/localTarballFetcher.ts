import path from 'node:path'

import type { FetchFunction, FetchOptions } from '@pnpm/fetching.fetcher-base'
import gfs from '@pnpm/fs.graceful-fs'
import type { Cafs } from '@pnpm/store.cafs-types'
import type { StoreIndex } from '@pnpm/store.index'
import { addFilesFromTarball } from '@pnpm/worker'

const isAbsolutePath = /^\/|^[A-Z]:/i

interface Resolution {
  integrity?: string
  registry?: string
  tarball: string
}

export function createLocalTarballFetcher (storeIndex: StoreIndex): FetchFunction {
  const fetch = (cafs: Cafs, resolution: Resolution, opts: FetchOptions) => {
    const tarball = resolvePath(opts.lockfileDir, resolution.tarball.slice(5))
    const buffer = gfs.readFileSync(tarball)
    return addFilesFromTarball({
      storeDir: cafs.storeDir,
      storeIndex,
      buffer,
      filesIndexFile: opts.filesIndexFile,
      integrity: resolution.integrity,
      readManifest: opts.readManifest,
      url: tarball,
      pkg: opts.pkg,
      appendManifest: opts.appendManifest,
      ignoreFilePattern: opts.ignoreFilePattern,
    })
  }

  return fetch as FetchFunction
}

function resolvePath (where: string, spec: string): string {
  if (isAbsolutePath.test(spec)) return spec
  return path.resolve(where, spec)
}
