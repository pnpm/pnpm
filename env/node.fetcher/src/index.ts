import fs from 'fs'
import path from 'path'
import { PnpmError } from '@pnpm/error'
import {
  type FetchFromRegistry,
  type RetryTimeoutOptions,
} from '@pnpm/fetching-types'
import { pickFetcher } from '@pnpm/pick-fetcher'
import { createCafsStore } from '@pnpm/create-cafs-store'
import { createTarballFetcher } from '@pnpm/tarball-fetcher'
import AdmZip from 'adm-zip'
import renameOverwrite from 'rename-overwrite'
import tempy from 'tempy'
import { isNonGlibcLinux } from 'detect-libc'
import { getNodeTarball } from './getNodeTarball'

export interface FetchNodeOptions {
  storeDir: string
  fetchTimeout?: number
  nodeMirrorBaseUrl?: string
  retry?: RetryTimeoutOptions
}

export async function fetchNode (fetch: FetchFromRegistry, version: string, targetDir: string, opts: FetchNodeOptions): Promise<void> {
  if (await isNonGlibcLinux()) {
    throw new PnpmError('MUSL', 'The current system uses the "MUSL" C standard library. Node.js currently has prebuilt artifacts only for the "glibc" libc, so we can install Node.js only for glibc')
  }
  const nodeMirrorBaseUrl = opts.nodeMirrorBaseUrl ?? 'https://nodejs.org/download/release/'
  const { tarball, pkgName } = getNodeTarball(version, nodeMirrorBaseUrl, process.platform, process.arch)
  if (tarball.endsWith('.zip')) {
    await downloadAndUnpackZip(fetch, tarball, targetDir, pkgName)
    return
  }
  const getAuthHeader = () => undefined
  const fetchers = createTarballFetcher(fetch, getAuthHeader, {
    retry: opts.retry,
    timeout: opts.fetchTimeout,
    // These are not needed for fetching Node.js
    rawConfig: {},
    unsafePerm: false,
  })
  const cafs = createCafsStore(opts.storeDir)
  const fetchTarball = pickFetcher(fetchers, { tarball })
  const { filesIndex } = await fetchTarball(cafs, { tarball } as any, { // eslint-disable-line @typescript-eslint/no-explicit-any
    filesIndexFile: path.join(opts.storeDir, encodeURIComponent(tarball)), // TODO: change the name or don't save an index file for node.js tarballs
    lockfileDir: process.cwd(),
    pkg: {},
  })
  cafs.importPackage(targetDir, {
    filesResponse: {
      filesIndex: filesIndex as Record<string, string>,
      resolvedFrom: 'remote',
      requiresBuild: false,
    },
    force: true,
  })
}

async function downloadAndUnpackZip (
  fetchFromRegistry: FetchFromRegistry,
  zipUrl: string,
  targetDir: string,
  pkgName: string
): Promise<void> {
  const response = await fetchFromRegistry(zipUrl)
  const tmp = path.join(tempy.directory(), 'pnpm.zip')
  const dest = fs.createWriteStream(tmp)
  await new Promise((resolve, reject) => {
    response.body!.pipe(dest).on('error', reject).on('close', resolve)
  })
  const zip = new AdmZip(tmp)
  const nodeDir = path.dirname(targetDir)
  zip.extractAllTo(nodeDir, true)
  await renameOverwrite(path.join(nodeDir, pkgName), targetDir)
  await fs.promises.unlink(tmp)
}
