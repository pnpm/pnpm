import ciInfo from 'ci-info'
import { fetch } from '@pnpm/fetch'
import { globalInfo } from '@pnpm/logger'
import enquirer from 'enquirer'
import { publish } from 'libnpmpublish'
import { type AuthTokenContext } from '../oidc/authToken.js'
import { type IdTokenContext } from '../oidc/idToken.js'
import { type ProvenanceContext } from '../oidc/provenance.js'
import { type OtpContext, type OtpPublishFn } from '../otp.js'

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
  // @types/libnpmpublish unfortunately uses an outdated type definition of package.json
  publish: publish as unknown as OtpPublishFn,
  setTimeout,
}
