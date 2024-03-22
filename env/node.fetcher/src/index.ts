import fs from 'node:fs'
import path from 'node:path'

import tempy from 'tempy'
import AdmZip from 'adm-zip'
import { isNonGlibcLinux } from 'detect-libc'
import renameOverwrite from 'rename-overwrite'

import type {
  FetchNodeOptions,
  FetchFromRegistry,
} from '@pnpm/types'
import { PnpmError } from '@pnpm/error'
import { pickFetcher } from '@pnpm/pick-fetcher'
import { createCafsStore } from '@pnpm/create-cafs-store'
import { createTarballFetcher } from '@pnpm/tarball-fetcher'

import { getNodeTarball } from './getNodeTarball.js'

export async function fetchNode(
  fetch: FetchFromRegistry,
  version: string,
  targetDir: string,
  opts: FetchNodeOptions
): Promise<void> {
  if (await isNonGlibcLinux()) {
    throw new PnpmError(
      'MUSL',
      'The current system uses the "MUSL" C standard library. Node.js currently has prebuilt artifacts only for the "glibc" libc, so we can install Node.js only for glibc'
    )
  }

  const nodeMirrorBaseUrl =
    opts.nodeMirrorBaseUrl ?? 'https://nodejs.org/download/release/'

  const { tarball, pkgName } = getNodeTarball(
    version,
    nodeMirrorBaseUrl,
    process.platform,
    process.arch
  )

  if (tarball.endsWith('.zip')) {
    await downloadAndUnpackZip(fetch, tarball, targetDir, pkgName)
    return
  }

  function getAuthHeader(): undefined {
    return undefined
  }

  const fetchers = createTarballFetcher(fetch, getAuthHeader, {
    retry: opts.retry,
    timeout: opts.fetchTimeout,
    // These are not needed for fetching Node.js
    rawConfig: {},
    unsafePerm: false,
  })

  const cafs = createCafsStore(opts.cafsDir)

  const fetchTarball = pickFetcher(fetchers, { tarball })

  // @ts-ignore
  const { filesIndex } = await fetchTarball(
    cafs,
    // @ts-ignore
    null,
    {
      filesIndexFile: path.join(opts.cafsDir, encodeURIComponent(tarball)), // TODO: change the name or don't save an index file for node.js tarballs
      lockfileDir: process.cwd(),
      pkg: {},
    }
  )

  cafs.importPackage(targetDir, {
    filesResponse: {
      filesIndex: filesIndex as Record<string, string>,
      resolvedFrom: 'remote',
    },
    force: true,
  })
}

async function downloadAndUnpackZip(
  fetchFromRegistry: FetchFromRegistry,
  zipUrl: string,
  targetDir: string,
  pkgName: string
): Promise<void> {
  const response = await fetchFromRegistry(zipUrl)

  const tmp = path.join(tempy.directory(), 'pnpm.zip')

  const dest = fs.createWriteStream(tmp)

  await new Promise((resolve, reject) => {
    response.body?.pipe(dest).on('error', reject).on('close', resolve)
  })

  const zip = new AdmZip(tmp)

  const nodeDir = path.dirname(targetDir)

  zip.extractAllTo(nodeDir, true)

  await renameOverwrite(path.join(nodeDir, pkgName), targetDir)

  await fs.promises.unlink(tmp)
}
