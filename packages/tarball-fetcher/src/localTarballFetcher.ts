import path from 'path'
import { Cafs, DeferredManifestPromise, FetchFunction, FetchOptions, FetchResult } from '@pnpm/fetcher-base'
import gfs from '@pnpm/graceful-fs'
import ssri from 'ssri'
import { TarballIntegrityError } from './remoteTarballFetcher'

const isAbsolutePath = /^[/]|^[A-Za-z]:/

interface Resolution {
  integrity?: string
  registry?: string
  tarball: string
}

export default function createLocalTarballFetcher (): FetchFunction {
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
  try {
    const tarballStream = gfs.createReadStream(tarball)
    const [fetchResult] = (
      await Promise.all([
        cafs.addFilesFromTarball(tarballStream, opts.manifest),
        opts.integrity && (ssri.checkStream(tarballStream, opts.integrity) as any), // eslint-disable-line
      ])
    )
    return { filesIndex: fetchResult }
  } catch (err: any) { // eslint-disable-line
    const error = new TarballIntegrityError({
      attempts: 1,
      algorithm: err['algorithm'],
      expected: err['expected'],
      found: err['found'],
      sri: err['sri'],
      url: tarball,
    })
    error['resource'] = tarball
    throw error
  }
}
