import { FetchFromRegistry } from '@pnpm/fetching-types'
import fetch, { RetryTimeoutOptions } from './fetch'
import createFetchFromRegistry, { AgentOptions } from './fetchFromRegistry'

export default fetch
export {
  AgentOptions,
  createFetchFromRegistry,
  FetchFromRegistry,
  RetryTimeoutOptions,
}
