import fs from 'fs'
import path from 'path'
import { PnpmError } from '@pnpm/error'
import {
  type FetchFromRegistry,
  type RetryTimeoutOptions,
} from '@pnpm/fetching-types'
import type { FilesIndex } from '@pnpm/cafs-types'
import { pickFetcher } from '@pnpm/pick-fetcher'
import { createCafsStore } from '@pnpm/create-cafs-store'
import { createTarballFetcher, waitForFilesIndex } from '@pnpm/tarball-fetcher'
import AdmZip from 'adm-zip'
import renameOverwrite from 'rename-overwrite'
import tempy from 'tempy'
import { isNonGlibcLinux } from 'detect-libc'
import { getNodeTarball } from './getNodeTarball'

export interface FetchNodeOptions {
  cafsDir: string
  fetchTimeout?: number
  nodeMirrorBaseUrl?: string
  retry?: RetryTimeoutOptions
}

export async function fetchNode (fetch: FetchFromRegistry, version: string, targetDir: string, opts: FetchNodeOptions) {
  // all node versions up until now (Aug 7 2023) strictly follow `vX.Y.Z` format
  // this code should be changed if the above condition ever ceases to apply
  if (!/^[0-9]+\.[0-9]+\.[0-9]+$/.test(version)) {
    throw new PnpmError('INVALID_NODE_VERSION', `"${version}" is not an exact version. The correct syntax is strictly X.Y.Z`)
  }
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
  const cafs = createCafsStore(opts.cafsDir)
  const fetchTarball = pickFetcher(fetchers, { tarball })
  const { filesIndex } = await fetchTarball(cafs, { tarball } as any, { // eslint-disable-line @typescript-eslint/no-explicit-any
    lockfileDir: process.cwd(),
  })
  await cafs.importPackage(targetDir, {
    filesResponse: {
      filesIndex: await waitForFilesIndex(filesIndex as FilesIndex),
      fromStore: false,
    },
    force: true,
  })
}

async function downloadAndUnpackZip (
  fetchFromRegistry: FetchFromRegistry,
  zipUrl: string,
  targetDir: string,
  pkgName: string
) {
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
