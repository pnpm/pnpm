import path from 'path'
import { type FetchFunction, type FetchOptions } from '@pnpm/fetcher-base'
import type { Cafs } from '@pnpm/cafs-types'
import gfs from '@pnpm/graceful-fs'
import { addFilesFromTarball } from '@pnpm/worker'

const isAbsolutePath = /^[/]|^[A-Za-z]:/

interface Resolution {
  integrity?: string
  registry?: string
  tarball: string
}

export function createLocalTarballFetcher (): FetchFunction {
  const fetch = (cafs: Cafs, resolution: Resolution, opts: FetchOptions) => {
    const tarball = resolvePath(opts.lockfileDir, resolution.tarball.slice(5))
    const buffer = gfs.readFileSync(tarball)
    return addFilesFromTarball({
      storeDir: cafs.storeDir,
      buffer,
      filesIndexFile: opts.filesIndexFile,
      integrity: resolution.integrity,
      readManifest: opts.readManifest,
      url: tarball,
      pkg: opts.pkg,
    })
  }

  return fetch as FetchFunction
}

function resolvePath (where: string, spec: string): string {
  if (isAbsolutePath.test(spec)) return spec
  return path.resolve(where, spec)
}
