import path from 'path'
import fsPromises from 'fs/promises'
import { PnpmError } from '@pnpm/error'
import { type FetchFromRegistry } from '@pnpm/fetching-types'
import { type ZipFetcher } from '@pnpm/fetcher-base'
import { addFilesFromDir } from '@pnpm/worker'
import AdmZip from 'adm-zip'
import renameOverwrite from 'rename-overwrite'
import tempy from 'tempy'
import ssri from 'ssri'

export function createZipFetcher (ctx: {
  fetch: FetchFromRegistry
  rawConfig: Record<string, string>
  offline?: boolean
}): { zip: ZipFetcher } {
  const fetchZip: ZipFetcher = async (cafs, resolution, opts) => {
    if (ctx.offline) {
      throw new PnpmError('CANNOT_DOWNLOAD_DENO_OFFLINE', 'Cannot download Deno because offline mode is enabled.')
    }
    const version = opts.pkg.version!

    const tempLocation = await cafs.tempDir()
    await downloadAndUnpackZip(ctx.fetch, {
      url: resolution.url,
      integrity: resolution.integrity,
      basename: '',
    }, tempLocation)
    const manifest = {
      name: opts.pkg.name!,
      version,
      bin: opts.pkg.name,
    }
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
    zip: fetchZip,
  }
}

export interface AssetInfo {
  url: string
  integrity: string
  basename: string
}

/**
 * Downloads and unpacks a zip file containing Node.js.
 *
 * @param fetchFromRegistry - Function to fetch resources from registry
 * @param artifactInfo - Information about the Node.js artifact
 * @param targetDir - Directory where Node.js should be installed
 * @throws {PnpmError} When integrity verification fails or extraction fails
 */
export async function downloadAndUnpackZip (
  fetchFromRegistry: FetchFromRegistry,
  artifactInfo: AssetInfo,
  targetDir: string
): Promise<void> {
  const tmp = path.join(tempy.directory(), 'pnpm.zip')

  try {
    await downloadWithIntegrityCheck(fetchFromRegistry, artifactInfo, tmp)
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
 */
async function downloadWithIntegrityCheck (
  fetchFromRegistry: FetchFromRegistry,
  { url, integrity }: AssetInfo,
  tmpPath: string
): Promise<void> {
  const response = await fetchFromRegistry(url)

  // Collect all chunks from the response
  const chunks: Buffer[] = []
  for await (const chunk of response.body!) {
    chunks.push(chunk as Buffer)
  }
  const data = Buffer.concat(chunks)

  try {
    // Verify integrity if provided
    ssri.checkData(data, integrity, { error: true })
  } catch (err) {
    if (!(err instanceof Error) || !('expected' in err) || !('found' in err)) {
      throw err
    }
    throw new PnpmError('TARBALL_INTEGRITY', `Got unexpected checksum for "${url}". Wanted "${err.expected as string}". Got "${err.found as string}".`)
  }

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
  const nodeDir = basename === '' ? targetDir : path.dirname(targetDir)
  const extractedDir = path.join(nodeDir, basename)

  zip.extractAllTo(nodeDir, true)
  await renameOverwrite(extractedDir, targetDir)
}
