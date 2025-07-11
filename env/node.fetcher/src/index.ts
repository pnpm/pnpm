import { createHash } from 'crypto'
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
  const tarball = getNodeTarball(version, nodeMirrorBaseUrl, process.platform, process.arch)
  const shasumsFileUrl = `${tarball.dirname}/SHASUMS256.txt`
  const tarballFileName = `${tarball.basename}${tarball.extname}`
  const integrity = await loadArtifactIntegrity(fetch, shasumsFileUrl, tarballFileName)
  const tarballUrl = `${tarball.dirname}/${tarballFileName}`
  if (tarball.extname === '.zip') {
    await downloadAndUnpackZip(fetch, tarballUrl, targetDir, tarballFileName, integrity)
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
  const fetchTarball = pickFetcher(fetchers, { tarball: tarballUrl })
  const { filesIndex } = await fetchTarball(cafs, { tarball: tarballUrl, integrity } as any, { // eslint-disable-line @typescript-eslint/no-explicit-any
    filesIndexFile: path.join(opts.storeDir, encodeURIComponent(tarballUrl)), // TODO: change the name or don't save an index file for node.js tarballs
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

async function loadArtifactIntegrity (fetch: FetchFromRegistry, integritiesFileUrl: string, fileName: string): Promise<string> {
  const res = await fetch(integritiesFileUrl)
  if (!res.ok) {
    throw new Error(`Failed to fetch integrity file: ${integritiesFileUrl} (status: ${res.status})`)
  }

  const body = await res.text()
  const line = body.split('\n').find(line => line.trim().endsWith(`  ${fileName}`))

  if (!line) {
    throw new Error(`SHA-256 hash not found in SHASUMS256.txt for: ${fileName}`)
  }

  const [sha256] = line.trim().split(/\s+/)
  if (!/^[a-f0-9]{64}$/.test(sha256)) {
    throw new Error(`Malformed SHA-256 for ${fileName}: ${sha256}`)
  }

  const buffer = Buffer.from(sha256, 'hex')
  const base64 = buffer.toString('base64')
  return `sha256-${base64}`
}

async function downloadAndUnpackZip (
  fetchFromRegistry: FetchFromRegistry,
  zipUrl: string,
  targetDir: string,
  pkgName: string,
  expectedIntegrity: string
): Promise<void> {
  const response = await fetchFromRegistry(zipUrl)
  const tmp = path.join(tempy.directory(), 'pnpm.zip')
  const dest = fs.createWriteStream(tmp)
  const hash = createHash('sha256')
  await new Promise((resolve, reject) => {
    response.body!.on('data', chunk => hash.update(chunk))
    response.body!.pipe(dest).on('error', reject).on('close', resolve)
  })
  const actual = `sha256-${hash.digest('base64')}`
  if (expectedIntegrity !== actual) {
    await fs.promises.unlink(tmp)
    throw new Error(
      `SHA-256 mismatch for ${zipUrl}\nExpected: ${expectedIntegrity}\nReceived: ${actual}`
    )
  }
  const zip = new AdmZip(tmp)
  const nodeDir = path.dirname(targetDir)
  zip.extractAllTo(nodeDir, true)
  await renameOverwrite(path.join(nodeDir, pkgName), targetDir)
  await fs.promises.unlink(tmp)
}
