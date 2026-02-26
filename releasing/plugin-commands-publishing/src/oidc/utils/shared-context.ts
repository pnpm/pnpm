import ciInfo from 'ci-info'
import { fetch } from '@pnpm/fetch'
import { globalInfo } from '@pnpm/logger'
import { type AuthTokenContext } from '../authToken.js'
import { type IdTokenContext } from '../idToken.js'
import { type ProvenanceContext } from '../provenance.js'

type SharedContext =
& AuthTokenContext
& IdTokenContext
& ProvenanceContext

export const SHARED_CONTEXT: SharedContext = {
  Date,
  ciInfo,
  fetch,
  globalInfo,
  process,
}
