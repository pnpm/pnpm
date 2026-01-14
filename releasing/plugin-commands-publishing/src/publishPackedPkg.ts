import fs from 'fs/promises'
import { type PublishOptions, publish } from 'libnpmpublish'
import { type Config } from '@pnpm/config'
import { FailedToPublishError } from './FailedToPublishError.js'
import { type PackResult } from './pack.js'

export type Options = Pick<Config,
| 'registries'
| 'sslConfigs'
| 'userAgent'
>

// @types/libnpmpublish unfortunately uses an outdated type definition of package.json
type OutdatedManifest = typeof publish extends (_a: infer Manifest, ..._: never) => unknown ? Manifest : never

export async function publishPackedPkg (packResult: PackResult, opts: Options): Promise<void> {
  const { publishedManifest, tarballPath } = packResult
  const tarballData = await fs.readFile(tarballPath)
  const response = await publish(publishedManifest as OutdatedManifest, tarballData, createPublishOptions(packResult, opts))
  if (response.ok) return
  throw await FailedToPublishError.createFailedToPublishError(packResult, response)
}

async function createPublishOptions (packResult: PackResult, opts: Options): Promise<PublishOptions> {
  throw new Error('TODO')
}
