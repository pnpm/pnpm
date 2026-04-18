import readline from 'node:readline'

import { globalInfo, globalWarn } from '@pnpm/logger'
import { fetch } from '@pnpm/network.fetch'
import type { ExportedManifest } from '@pnpm/releasing.exportable-manifest'
import ciInfo from 'ci-info'
import enquirer from 'enquirer'
import { publish as _publish, type PublishOptions } from 'libnpmpublish'

import type { AuthTokenContext } from '../oidc/authToken.js'
import type { IdTokenContext } from '../oidc/idToken.js'
import type { ProvenanceContext } from '../oidc/provenance.js'
import type { OtpContext } from '../otp.js'

// @types/libnpmpublish uses an outdated PackageJson type that is incompatible
// with ExportedManifest. This intermediate type bridges only that manifest
// parameter difference while preserving the rest of the original signature.
type PublishWithExportedManifest = (
  manifest: ExportedManifest,
  tarballData: Buffer,
  options: PublishOptions
) => ReturnType<typeof _publish>
const publish = _publish as PublishWithExportedManifest

type SharedContext =
& AuthTokenContext
& IdTokenContext
& ProvenanceContext
& OtpContext

export const SHARED_CONTEXT: SharedContext = {
  Date,
  createReadlineInterface: readline.createInterface.bind(null, { input: process.stdin }),
  ciInfo,
  enquirer,
  fetch,
  globalInfo,
  globalWarn,
  process,
  publish,
  setTimeout,
}
