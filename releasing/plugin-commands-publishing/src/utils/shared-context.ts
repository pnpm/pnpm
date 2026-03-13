import ciInfo from 'ci-info'
import { fetch } from '@pnpm/fetch'
import { globalInfo } from '@pnpm/logger'
import enquirer from 'enquirer'
import { type ExportedManifest } from '@pnpm/exportable-manifest'
import { publish, type PublishOptions } from 'libnpmpublish'
import { type AuthTokenContext } from '../oidc/authToken.js'
import { type IdTokenContext } from '../oidc/idToken.js'
import { type ProvenanceContext } from '../oidc/provenance.js'
import { type OtpContext, type OtpEnquirer, type OtpPublishFn } from '../otp.js'

// @types/libnpmpublish uses an outdated PackageJson type that is incompatible
// with ExportedManifest. This intermediate type bridges only that manifest
// parameter difference while preserving the rest of the original signature.
type PublishWithExportedManifest = (
  manifest: ExportedManifest,
  tarballData: Buffer,
  options: PublishOptions
) => ReturnType<typeof publish>

type SharedContext =
& AuthTokenContext
& IdTokenContext
& ProvenanceContext
& OtpContext

export const SHARED_CONTEXT: SharedContext = {
  Date,
  ciInfo,
  enquirer: enquirer as unknown as OtpEnquirer,
  fetch,
  globalInfo,
  process,
  publish: publish as PublishWithExportedManifest as OtpPublishFn,
  setTimeout,
}
