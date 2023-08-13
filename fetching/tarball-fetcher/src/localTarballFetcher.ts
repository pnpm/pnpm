import path from 'path'
import { type FetchFunction, type FetchOptions, type FetchResult } from '@pnpm/fetcher-base'
import type { Cafs, DeferredManifestPromise } from '@pnpm/cafs-types'
import gfs from '@pnpm/graceful-fs'
import ssri from 'ssri'
import { TarballIntegrityError } from './remoteTarballFetcher'

const isAbsolutePath = /^[/]|^[A-Za-z]:/

interface Resolution {
  integrity?: string
  registry?: string
  tarball: string
}

export function createLocalTarballFetcher (): FetchFunction {
  const fetch = (cafs: Cafs, resolution: Resolution, opts: FetchOptions) => {
    const tarball = resolvePath(opts.lockfileDir, resolution.tarball.slice(5))

    return fetchFromLocalTarball(cafs, tarball, {
      integrity: resolution.integrity,
      manifest: opts.manifest,
    })
  }

  return fetch as FetchFunction
}

function resolvePath (where: string, spec: string) {
  if (isAbsolutePath.test(spec)) return spec
  return path.resolve(where, spec)
}

async function fetchFromLocalTarball (
  cafs: Cafs,
  tarball: string,
  opts: {
    integrity?: string
    manifest?: DeferredManifestPromise
  }
): Promise<FetchResult> {
  const tarballBuffer = gfs.readFileSync(tarball)
  if (opts.integrity) {
    try {
      ssri.checkData(tarballBuffer, opts.integrity, { error: true })
    } catch (err: any) { // eslint-disable-line
      const error = new TarballIntegrityError({
        attempts: 1,
        algorithm: err['algorithm'],
        expected: err['expected'],
        found: err['found'],
        sri: err['sri'],
        url: tarball,
      })
      // @ts-expect-error
      error['resource'] = tarball
      throw error
    }
  }
  const filesIndex = cafs.addFilesFromTarball(tarballBuffer, opts.manifest)
  return { unprocessed: true, filesIndex }
}
