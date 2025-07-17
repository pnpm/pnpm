import fsPromises from 'fs/promises'
import path from 'path'
import { PnpmError } from '@pnpm/error'
import {
  type FetchFromRegistry,
  type RetryTimeoutOptions,
  type Response,
} from '@pnpm/fetching-types'
import { pickFetcher } from '@pnpm/pick-fetcher'
import { createCafsStore } from '@pnpm/create-cafs-store'
import { createHash } from '@pnpm/crypto.hash'
import { type Cafs } from '@pnpm/cafs-types'
import { createTarballFetcher } from '@pnpm/tarball-fetcher'
import { type NodeRuntimeFetcher, type FetchFunction, type FetchResult } from '@pnpm/fetcher-base'
import { getNodeMirror, parseEnvSpecifier } from '@pnpm/node.resolver'
import { addFilesFromDir } from '@pnpm/worker'
import AdmZip from 'adm-zip'
import renameOverwrite from 'rename-overwrite'
import tempy from 'tempy'
import { isNonGlibcLinux } from 'detect-libc'
import ssri from 'ssri'
import { getNodeArtifactAddress } from './getNodeArtifactAddress'

export function createNodeRuntimeFetcher (ctx: {
  fetch: FetchFromRegistry
  rawConfig: Record<string, string>
  offline?: boolean
}): { nodeRuntime: NodeRuntimeFetcher } {
  const fetchNodeRuntime: NodeRuntimeFetcher = async (cafs, resolution, opts) => {
    if (!opts.pkg.version && !opts.pkg.id) {
      throw new Error('Cannot fetch node.js without a version')
    }
    if (ctx.offline) {
      throw new PnpmError('CANNOT_DOWNLOAD_NODE_OFFLINE', 'Cannot download Node.js because offline mode is enabled.')
    }
    const version = opts.pkg.version ?? opts.pkg.id.replace('runtime:', '')
    const { releaseChannel } = parseEnvSpecifier(version)

    await validateSystemCompatibility()

    const nodeMirrorBaseUrl = getNodeMirror(ctx.rawConfig, releaseChannel)
    const artifactInfo = await getNodeArtifactInfo(ctx.fetch, version, {
      nodeMirrorBaseUrl,
      expectedVersionIntegrity: resolution.integrity,
      cachedShasumsFile: resolution._shasumsFileContent,
    })
    const manifest = {
      name: 'node',
      version,
      bin: process.platform === 'win32' ? 'node.exe' : 'bin/node',
    }

    if (artifactInfo.isZip) {
      const tempLocation = await cafs.tempDir()
      await downloadAndUnpackZip(ctx.fetch, artifactInfo, tempLocation)
      return {
        ...await addFilesFromDir({
          storeDir: cafs.storeDir,
          dir: tempLocation,
          filesIndexFile: opts.filesIndexFile,
          readManifest: false,
        }),
        manifest,
      }
    }

    return {
      ...await downloadAndUnpackTarball(ctx.fetch, artifactInfo, { cafs, filesIndexFile: opts.filesIndexFile }),
      manifest,
    }
  }
  return {
    nodeRuntime: fetchNodeRuntime,
  }
}

// Constants
const DEFAULT_NODE_MIRROR_BASE_URL = 'https://nodejs.org/download/release/'
const SHA256_REGEX = /^[a-f0-9]{64}$/

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
    expectedVersionIntegrity?: string
    cachedShasumsFile?: string
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

  const integrity = opts.cachedShasumsFile ? pickArtifactIntegrity(opts.cachedShasumsFile, tarballFileName) : await loadArtifactIntegrity(fetch, shasumsFileUrl, tarballFileName)

  return {
    url,
    integrity,
    isZip: tarball.extname === '.zip',
    basename: tarball.basename,
  }
}

/**
 * Loads and verifies the integrity hash for a Node.js artifact.
 *
 * @param fetch - Function to fetch resources from registry
 * @param integritiesFileUrl - URL of the SHASUMS256.txt file
 * @param fileName - Name of the file to find integrity for
 * @returns Promise resolving to the integrity hash in base64 format
 * @throws {PnpmError} When integrity file cannot be fetched or parsed
 */
async function loadArtifactIntegrity (
  fetch: FetchFromRegistry,
  integritiesFileUrl: string,
  fileName: string,
  expectedVersionIntegrity?: string
): Promise<string> {
  const res = await fetch(integritiesFileUrl)
  if (!res.ok) {
    throw new PnpmError(
      'NODE_FETCH_INTEGRITY_FAILED',
      `Failed to fetch integrity file: ${integritiesFileUrl} (status: ${res.status})`
    )
  }

  const body = await res.text()
  if (expectedVersionIntegrity) {
    const actualVersionIntegrity = createHash(body)
    if (expectedVersionIntegrity !== actualVersionIntegrity) {
      throw new PnpmError('NODE_VERSION_INTEGRITY_MISMATCH', `The integrity of ${integritiesFileUrl} failed. Expected: ${expectedVersionIntegrity}. Actual: ${actualVersionIntegrity}`)
    }
  }
  return pickArtifactIntegrity(body, fileName)
}

function pickArtifactIntegrity (body: string, fileName: string): string {
  const line = body.split('\n').find(line => line.trim().endsWith(`  ${fileName}`))

  if (!line) {
    throw new PnpmError(
      'NODE_INTEGRITY_HASH_NOT_FOUND',
      `SHA-256 hash not found in SHASUMS256.txt for: ${fileName}`
    )
  }

  const [sha256] = line.trim().split(/\s+/)
  if (!SHA256_REGEX.test(sha256)) {
    throw new PnpmError(
      'NODE_MALFORMED_INTEGRITY_HASH',
      `Malformed SHA-256 for ${fileName}: ${sha256}`
    )
  }

  const buffer = Buffer.from(sha256, 'hex')
  const base64 = buffer.toString('base64')
  return `sha256-${base64}`
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
  const fetchTarball = pickFetcher(fetchers, { tarball: artifactInfo.url }) as FetchFunction

  // Create a unique index file name for Node.js tarballs
  const indexFileName = `node-${encodeURIComponent(artifactInfo.url)}`
  const filesIndexFile = path.join(opts.storeDir, indexFileName)

  const { filesIndex } = await fetchTarball(cafs, {
    tarball: artifactInfo.url,
    integrity: artifactInfo.integrity,
  }, {
    filesIndexFile,
    lockfileDir: process.cwd(),
    pkg: {
      id: '',
    },
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

/**
 * Downloads and unpacks a tarball using the tarball fetcher.
 *
 * @param fetch - Function to fetch resources from registry
 * @param artifactInfo - Information about the Node.js artifact
 * @param targetDir - Directory where Node.js should be installed
 * @param opts - Configuration options for the fetch operation
 */
async function downloadAndUnpackTarball (
  fetch: FetchFromRegistry,
  artifactInfo: NodeArtifactInfo,
  opts: FetchNodeOptions
): Promise<FetchResult> {
  const getAuthHeader = () => undefined
  const fetchers = createTarballFetcher(fetch, getAuthHeader, {
    retry: opts.retry,
    timeout: opts.fetchTimeout,
    // These are not needed for fetching Node.js
    rawConfig: {},
    unsafePerm: false,
  })

  const fetchTarball = pickFetcher(fetchers, { tarball: artifactInfo.url }) as FetchFunction

  return fetchTarball(opts.cafs, {
    tarball: artifactInfo.url,
    integrity: artifactInfo.integrity,
  }, {
    filesIndexFile: opts.filesIndexFile,
    lockfileDir: process.cwd(),
    pkg: {
      id: '',
    },
  })
}

/**
 * Downloads and unpacks a zip file containing Node.js.
 *
 * @param fetchFromRegistry - Function to fetch resources from registry
 * @param artifactInfo - Information about the Node.js artifact
 * @param targetDir - Directory where Node.js should be installed
 * @throws {PnpmError} When integrity verification fails or extraction fails
 */
async function downloadAndUnpackZip (
  fetchFromRegistry: FetchFromRegistry,
  artifactInfo: NodeArtifactInfo,
  targetDir: string
): Promise<void> {
  const response = await fetchFromRegistry(artifactInfo.url)
  const tmp = path.join(tempy.directory(), 'pnpm.zip')

  try {
    await downloadWithIntegrityCheck(response, tmp, artifactInfo.integrity)
    await extractZipToTarget(tmp, artifactInfo.basename, targetDir)
  } finally {
    // Clean up temporary file
    try {
      await fsPromises.unlink(tmp)
    } catch {
      // Ignore cleanup errors
    }
  }
}

/**
 * Downloads a file with integrity verification.
 *
 * @param response - Fetch response containing the file data
 * @param tmpPath - Temporary file path to save the download
 * @param expectedIntegrity - Expected SHA-256 integrity hash
 * @param url - URL being downloaded (for error messages)
 * @throws {PnpmError} When integrity verification fails
 */
async function downloadWithIntegrityCheck (
  response: Response,
  tmpPath: string,
  expectedIntegrity: string
): Promise<void> {
  // Collect all chunks from the response
  const chunks: Buffer[] = []
  for await (const chunk of response.body!) {
    chunks.push(chunk as Buffer)
  }
  const data = Buffer.concat(chunks)

  // Verify integrity if provided
  ssri.checkData(data, expectedIntegrity, { error: true })

  // Write the verified data to file
  await fsPromises.writeFile(tmpPath, data)
}

/**
 * Extracts a zip file to the target directory.
 *
 * @param zipPath - Path to the zip file
 * @param basename - Base name of the file (without extension)
 * @param targetDir - Directory where contents should be extracted
 * @throws {PnpmError} When extraction fails
 */
async function extractZipToTarget (
  zipPath: string,
  basename: string,
  targetDir: string
): Promise<void> {
  const zip = new AdmZip(zipPath)
  const nodeDir = path.dirname(targetDir)
  const extractedDir = path.join(nodeDir, basename)

  zip.extractAllTo(nodeDir, true)
  await renameOverwrite(extractedDir, targetDir)
}
