import path from 'path'
import { PnpmError } from '@pnpm/error'
import { fetchShasumsFileRaw, pickFileChecksumFromShasumsFile } from '@pnpm/crypto.shasums-file'
import {
  type FetchFromRegistry,
  type RetryTimeoutOptions,
} from '@pnpm/fetching-types'
import { createCafsStore } from '@pnpm/create-cafs-store'
import { type Cafs } from '@pnpm/cafs-types'
import { createTarballFetcher } from '@pnpm/tarball-fetcher'
import { getNodeArtifactAddress } from '@pnpm/node.resolver'
import { downloadAndUnpackZip } from '@pnpm/fetching.binary-fetcher'
import { isNonGlibcLinux } from 'detect-libc'

// Constants
const DEFAULT_NODE_MIRROR_BASE_URL = 'https://nodejs.org/download/release/'

export interface FetchNodeOptionsToDir {
  storeDir: string
  fetchTimeout?: number
  nodeMirrorBaseUrl?: string
  retry?: RetryTimeoutOptions
}

export interface FetchNodeOptions {
  cafs: Cafs
  filesIndexFile: string
  fetchTimeout?: number
  nodeMirrorBaseUrl?: string
  retry?: RetryTimeoutOptions
}

interface NodeArtifactInfo {
  url: string
  integrity: string
  isZip: boolean
  basename: string
}

/**
 * Fetches and installs a Node.js version to the specified target directory.
 *
 * @param fetch - Function to fetch resources from registry
 * @param version - Node.js version to install
 * @param targetDir - Directory where Node.js should be installed
 * @param opts - Configuration options for the fetch operation
 * @throws {PnpmError} When system uses MUSL libc, integrity verification fails, or download fails
 */
export async function fetchNode (
  fetch: FetchFromRegistry,
  version: string,
  targetDir: string,
  opts: FetchNodeOptionsToDir
): Promise<void> {
  await validateSystemCompatibility()

  const nodeMirrorBaseUrl = opts.nodeMirrorBaseUrl ?? DEFAULT_NODE_MIRROR_BASE_URL
  const artifactInfo = await getNodeArtifactInfo(fetch, version, { nodeMirrorBaseUrl })

  if (artifactInfo.isZip) {
    await downloadAndUnpackZip(fetch, artifactInfo, targetDir)
    return
  }

  await downloadAndUnpackTarballToDir(fetch, artifactInfo, targetDir, opts)
}

/**
 * Validates that the current system is compatible with Node.js installation.
 *
 * @throws {PnpmError} When system uses MUSL libc
 */
async function validateSystemCompatibility (): Promise<void> {
  if (await isNonGlibcLinux()) {
    throw new PnpmError(
      'MUSL',
      'The current system uses the "MUSL" C standard library. Node.js currently has prebuilt artifacts only for the "glibc" libc, so we can install Node.js only for glibc'
    )
  }
}

/**
 * Gets Node.js artifact information including URL, integrity, and file type.
 *
 * @param fetch - Function to fetch resources from registry
 * @param version - Node.js version
 * @param nodeMirrorBaseUrl - Base URL for Node.js mirror
 * @returns Promise resolving to artifact information
 * @throws {PnpmError} When integrity file cannot be fetched or parsed
 */
async function getNodeArtifactInfo (
  fetch: FetchFromRegistry,
  version: string,
  opts: {
    nodeMirrorBaseUrl: string
    integrities?: Record<string, string>
  }
): Promise<NodeArtifactInfo> {
  const tarball = getNodeArtifactAddress({
    version,
    baseUrl: opts.nodeMirrorBaseUrl,
    platform: process.platform,
    arch: process.arch,
  })

  const tarballFileName = `${tarball.basename}${tarball.extname}`
  const shasumsFileUrl = `${tarball.dirname}/SHASUMS256.txt`
  const url = `${tarball.dirname}/${tarballFileName}`

  const integrity = opts.integrities
    ? opts.integrities[`${process.platform}-${process.arch}`]
    : await loadArtifactIntegrity(fetch, tarballFileName, shasumsFileUrl)

  return {
    url,
    integrity,
    isZip: tarball.extname === '.zip',
    basename: tarball.basename,
  }
}

/**
 * Loads and extracts the integrity hash for a specific Node.js artifact.
 *
 * @param fetch - Function to fetch resources from registry
 * @param fileName - Name of the file to find integrity for
 * @param shasumsUrl - URL of the SHASUMS256.txt file
 * @param options - Optional configuration for integrity verification
 * @returns Promise resolving to the integrity hash in base64 format
 * @throws {PnpmError} When integrity file cannot be fetched or parsed
 */
async function loadArtifactIntegrity (
  fetch: FetchFromRegistry,
  fileName: string,
  shasumsUrl: string
): Promise<string> {
  const body = await fetchShasumsFileRaw(fetch, shasumsUrl)
  return pickFileChecksumFromShasumsFile(body, fileName)
}

/**
 * Downloads and unpacks a tarball using the tarball fetcher.
 *
 * @param fetch - Function to fetch resources from registry
 * @param artifactInfo - Information about the Node.js artifact
 * @param targetDir - Directory where Node.js should be installed
 * @param opts - Configuration options for the fetch operation
 */
async function downloadAndUnpackTarballToDir (
  fetch: FetchFromRegistry,
  artifactInfo: NodeArtifactInfo,
  targetDir: string,
  opts: FetchNodeOptionsToDir
): Promise<void> {
  const getAuthHeader = () => undefined
  const fetchers = createTarballFetcher(fetch, getAuthHeader, {
    retry: opts.retry,
    timeout: opts.fetchTimeout,
    // These are not needed for fetching Node.js
    rawConfig: {},
    unsafePerm: false,
  })

  const cafs = createCafsStore(opts.storeDir)

  // Create a unique index file name for Node.js tarballs
  const indexFileName = `node-${encodeURIComponent(artifactInfo.url)}`
  const filesIndexFile = path.join(opts.storeDir, indexFileName)

  const { filesIndex } = await fetchers.remoteTarball(cafs, {
    tarball: artifactInfo.url,
    integrity: artifactInfo.integrity,
  }, {
    filesIndexFile,
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
