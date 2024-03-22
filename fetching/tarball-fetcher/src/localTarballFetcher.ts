import path from 'node:path'

import gfs from '@pnpm/graceful-fs'
import { addFilesFromTarball } from '@pnpm/worker'
import type { Cafs, DependencyManifest, FetchOptions } from '@pnpm/types'

const isAbsolutePath = /^[/]|^[A-Za-z]:/

export function createLocalTarballFetcher(): (cafs: Cafs, resolution: {
  integrity?: string | undefined
  registry?: string | undefined
  tarball: string
}, opts: FetchOptions) => Promise<{
  filesIndex: Record<string, string>;
  manifest: DependencyManifest;
}> {
  return (cafs: Cafs, resolution: {
    integrity?: string | undefined
    registry?: string | undefined
    tarball: string
  }, opts: FetchOptions): Promise<{
    filesIndex: Record<string, string>;
    manifest: DependencyManifest;
  }> => {
    const tarball = resolvePath(opts.lockfileDir, resolution.tarball.slice(5))

    const buffer = gfs.default.readFileSync(tarball)

    return addFilesFromTarball({
      cafsDir: cafs.cafsDir,
      buffer,
      filesIndexFile: opts.filesIndexFile,
      integrity: resolution.integrity,
      readManifest: opts.readManifest,
      url: tarball,
      pkg: opts.pkg,
    })
  }
}

function resolvePath(where: string, spec: string): string {
  if (isAbsolutePath.test(spec)) {
    return spec
  }

  return path.resolve(where, spec)
}
