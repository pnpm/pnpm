import { FetchFromRegistry } from '@pnpm/fetching-types'
import fetch, { RetryTimeoutOptions } from './fetch'
import createFetchFromRegistry, { fetchWithAgent, AgentOptions } from './fetchFromRegistry'

export {
  fetch,
  AgentOptions,
  createFetchFromRegistry,
  fetchWithAgent,
  FetchFromRegistry,
  RetryTimeoutOptions,
}
