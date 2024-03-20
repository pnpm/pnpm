import '@total-typescript/ts-reset'

import { type Logger, logger } from '@pnpm/logger'
import type { ContextMessage, DeprecationMessage, ExecutionTimeMessage, FetchingProgressMessage, HookMessage, InstallCheckMessage, LifecycleMessage, LinkMessage, PackageImportMethodMessage, PackageManifestMessage, PeerDependencyIssuesMessage, ProgressMessage, RegistryMessage, RequestRetryMessage, RootMessage, ScopeMessage, SkippedOptionalDependencyMessage, StageMessage, StatsMessage, SummaryMessage, UpdateCheckMessage } from '@pnpm/types'

export const contextLogger: Logger<ContextMessage> = logger<ContextMessage>('context')

export const deprecationLogger: Logger<DeprecationMessage> = logger<DeprecationMessage>(
  'deprecation'
)

export const executionTimeLogger: Logger<ExecutionTimeMessage> = logger<ExecutionTimeMessage>('execution-time')

export const fetchingProgressLogger: Logger<FetchingProgressMessage> = logger<FetchingProgressMessage>(
  'fetching-progress'
)

export const hookLogger: Logger<HookMessage> = logger<HookMessage>('hook')

export const installCheckLogger: Logger<InstallCheckMessage> = logger<InstallCheckMessage>('install-check')

export const lifecycleLogger: Logger<LifecycleMessage> = logger<LifecycleMessage>('lifecycle')

export const packageImportMethodLogger: Logger<PackageImportMethodMessage> = logger<PackageImportMethodMessage>('package-import-method')

export const linkLogger: Logger<LinkMessage> = logger<LinkMessage>('link')

export const packageManifestLogger: Logger<PackageManifestMessage> =
  logger<PackageManifestMessage>('package-manifest')

export const peerDependencyIssuesLogger: Logger<PeerDependencyIssuesMessage> = logger<PeerDependencyIssuesMessage>(
  'peer-dependency-issues'
)

export const progressLogger: Logger<ProgressMessage> = logger<ProgressMessage>('progress')

export const registryLogger: Logger<RegistryMessage> = logger<RegistryMessage>('progress')

export const removalLogger: Logger<string> = logger<string>('removal')

export const requestRetryLogger: Logger<RequestRetryMessage> = logger<RequestRetryMessage>('request-retry')

export const rootLogger: Logger<RootMessage> = logger<RootMessage>('root')

export const scopeLogger: Logger<ScopeMessage> = logger<ScopeMessage>('scope')

export const skippedOptionalDependencyLogger: Logger<SkippedOptionalDependencyMessage> =
  logger<SkippedOptionalDependencyMessage>('skipped-optional-dependency')

export const stageLogger: Logger<StageMessage> = logger<StageMessage>('stage')

export const statsLogger: Logger<StatsMessage> = logger<StatsMessage>('stats')

export const summaryLogger: Logger<SummaryMessage> = logger<SummaryMessage>('summary')

export const updateCheckLogger: Logger<UpdateCheckMessage> = logger<UpdateCheckMessage>('update-check')
