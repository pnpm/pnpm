import path from 'path'
import { fetchShasumsFileRaw, pickFileChecksumFromShasumsFile } from '@pnpm/crypto.shasums-file'
import {
  type FetchFromRegistry,
  type RetryTimeoutOptions,
} from '@pnpm/fetching-types'
import { createCafsStore } from '@pnpm/create-cafs-store'
import { type Cafs } from '@pnpm/cafs-types'
import { createTarballFetcher } from '@pnpm/tarball-fetcher'
import {
  getNodeArtifactAddress,
  DEFAULT_NODE_MIRROR_BASE_URL,
  UNOFFICIAL_NODE_MIRROR_BASE_URL,
} from '@pnpm/node.resolver'
import { downloadAndUnpackZip } from '@pnpm/fetching.binary-fetcher'
import { isNonGlibcLinux } from 'detect-libc'

export interface FetchNodeOptionsToDir {
  storeDir: string
  fetchTimeout?: number
  nodeMirrorBaseUrl?: string
  retry?: RetryTimeoutOptions
  /** Target platform (e.g. 'linux', 'darwin', 'win32'). Defaults to the current host platform. */
  platform?: string
  /** Target CPU architecture (e.g. 'x64', 'arm64'). Defaults to the current host arch. */
  arch?: string
  /** C standard library variant. Use 'musl' to download musl (static) Linux builds
   * from unofficial-builds.nodejs.org. Defaults to auto-detected on the host. */
  libc?: string
}

export interface FetchNodeOptions {
  cafs: Cafs
  filesIndexFile: string
  fetchTimeout?: number
  nodeMirrorBaseUrl?: string
  retry?: RetryTimeoutOptions
  platform?: string
  arch?: string
  libc?: string
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
 * @throws {PnpmError} When integrity verification fails or download fails
 */
export async function fetchNode (
  fetch: FetchFromRegistry,
  version: string,
  targetDir: string,
  opts: FetchNodeOptionsToDir
): Promise<void> {
  const platform = opts.platform ?? process.platform
  const arch = opts.arch ?? process.arch
  // On a native musl system with no explicit libc, automatically use the musl
  // variant so that pnpm env works out of the box on Alpine Linux and similar.
  // When an explicit platform override is set (cross-platform build), trust the
  // caller to pass the correct libc; do not auto-detect from the host.
  const libc = opts.libc ?? (!opts.platform && await isNonGlibcLinux() ? 'musl' : undefined)

  const isMusl = libc === 'musl'
  const nodeMirrorBaseUrl = opts.nodeMirrorBaseUrl ?? (isMusl
    ? UNOFFICIAL_NODE_MIRROR_BASE_URL
    : DEFAULT_NODE_MIRROR_BASE_URL)

  const artifactInfo = await getNodeArtifactInfo(fetch, version, {
    nodeMirrorBaseUrl,
    platform,
    arch,
    libc,
  })

  if (artifactInfo.isZip) {
    await downloadAndUnpackZip(fetch, artifactInfo, targetDir)
    return
  }

  await downloadAndUnpackTarballToDir(fetch, artifactInfo, targetDir, opts)
}

/**
 * Gets Node.js artifact information including URL, integrity, and file type.
 *
 * @param fetch - Function to fetch resources from registry
 * @param version - Node.js version
 * @param opts - Options including nodeMirrorBaseUrl, platform, arch, and libc
 * @returns Promise resolving to artifact information
 * @throws {PnpmError} When integrity file cannot be fetched or parsed
 */
async function getNodeArtifactInfo (
  fetch: FetchFromRegistry,
  version: string,
  opts: {
    nodeMirrorBaseUrl: string
    integrities?: Record<string, string>
    platform: string
    arch: string
    libc?: string
  }
): Promise<NodeArtifactInfo> {
  const tarball = getNodeArtifactAddress({
    version,
    baseUrl: opts.nodeMirrorBaseUrl,
    platform: opts.platform,
    arch: opts.arch,
    libc: opts.libc,
  })

  const tarballFileName = `${tarball.basename}${tarball.extname}`
  const shasumsFileUrl = `${tarball.dirname}/SHASUMS256.txt`
  const url = `${tarball.dirname}/${tarballFileName}`

  const isMusl = opts.libc === 'musl'
  const integrityKey = isMusl ? `${opts.platform}-${opts.arch}-musl` : `${opts.platform}-${opts.arch}`
  const integrity = opts.integrities
    ? opts.integrities[integrityKey]
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

  const { filesMap } = await fetchers.remoteTarball(cafs, {
    tarball: artifactInfo.url,
    integrity: artifactInfo.integrity,
  }, {
    filesIndexFile,
    lockfileDir: process.cwd(),
    pkg: {},
  })

  cafs.importPackage(targetDir, {
    filesResponse: {
      filesMap,
      resolvedFrom: 'remote',
      requiresBuild: false,
    },
    force: true,
  })
}
