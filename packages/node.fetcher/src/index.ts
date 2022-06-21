import fs from 'fs'
import path from 'path'
import { Config } from '@pnpm/config'
import { FetchFromRegistry } from '@pnpm/fetch'
import { FilesIndex } from '@pnpm/fetcher-base'
import createCafsStore from '@pnpm/create-cafs-store'
import storePath from '@pnpm/store-path'
import createFetcher, { waitForFilesIndex } from '@pnpm/tarball-fetcher'
import AdmZip from 'adm-zip'
import renameOverwrite from 'rename-overwrite'
import tempy from 'tempy'
import { getNodeTarball } from './getNodeTarball'

export type FetchNodeOptions = Pick<Config,
| 'fetchRetries'
| 'fetchRetryFactor'
| 'fetchRetryMaxtimeout'
| 'fetchRetryMintimeout'
| 'fetchTimeout'
| 'storeDir'
| 'pnpmHomeDir'
> & {
  nodeMirrorBaseUrl: string
}

export async function fetchNode (fetch: FetchFromRegistry, version: string, targetDir: string, opts: FetchNodeOptions) {
  const { tarball, pkgName } = getNodeTarball(version, opts.nodeMirrorBaseUrl, process.platform, process.arch)
  if (tarball.endsWith('.zip')) {
    await downloadAndUnpackZip(fetch, tarball, targetDir, pkgName)
    return
  }
  const getCredentials = () => ({ authHeaderValue: undefined, alwaysAuth: undefined })
  const { tarball: fetchTarball } = createFetcher(fetch, getCredentials, {
    retry: {
      maxTimeout: opts.fetchRetryMaxtimeout,
      minTimeout: opts.fetchRetryMintimeout,
      retries: opts.fetchRetries,
      factor: opts.fetchRetryFactor,
    },
    timeout: opts.fetchTimeout,
  })
  const storeDir = await storePath({
    pkgRoot: process.cwd(),
    storePath: opts.storeDir,
    pnpmHomeDir: opts.pnpmHomeDir,
  })
  const cafsDir = path.join(storeDir, 'files')
  const cafs = createCafsStore(cafsDir)
  const { filesIndex } = await fetchTarball(cafs, { tarball }, {
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
