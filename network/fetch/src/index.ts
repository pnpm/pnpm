import '@total-typescript/ts-reset'

export type { FetchFromRegistry } from '@pnpm/fetching-types'

export {
  fetchWithAgent,
  type AgentOptions,
  createFetchFromRegistry,
} from './fetchFromRegistry'
export { fetch } from './fetch'
