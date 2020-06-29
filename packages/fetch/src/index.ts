import { FetchFromRegistry } from '@pnpm/fetching-types'
import fetch, { RetryTimeoutOptions } from './fetch'
import createFetchFromRegistry from './fetchFromRegistry'

export default fetch
export { createFetchFromRegistry, FetchFromRegistry, RetryTimeoutOptions }
