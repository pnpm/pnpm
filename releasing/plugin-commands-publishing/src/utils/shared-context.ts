import ciInfo from 'ci-info'
import { fetch } from '@pnpm/fetch'
import { globalInfo } from '@pnpm/logger'
import enquirer from 'enquirer'
import { type ExportedManifest } from '@pnpm/exportable-manifest'
import { publish as _publish, type PublishOptions } from 'libnpmpublish'
import { type AuthTokenContext } from '../oidc/authToken.js'
import { type IdTokenContext } from '../oidc/idToken.js'
import { type ProvenanceContext } from '../oidc/provenance.js'
import { type OtpContext, type OtpPublishFn } from '../otp.js'

// @types/libnpmpublish uses an outdated PackageJson type that is incompatible
// with ExportedManifest. This intermediate type bridges only that manifest
// parameter difference while preserving the rest of the original signature.
type PublishWithExportedManifest = (
  manifest: ExportedManifest,
  tarballData: Buffer,
  options: PublishOptions
) => ReturnType<typeof _publish>
const publish = _publish as PublishWithExportedManifest as OtpPublishFn

type SharedContext =
& AuthTokenContext
& IdTokenContext
& ProvenanceContext
& OtpContext

export const SHARED_CONTEXT: SharedContext = {
  Date,
  ciInfo,
  enquirer,
  fetch,
  globalInfo,
  process,
  publish,
  setTimeout,
}
