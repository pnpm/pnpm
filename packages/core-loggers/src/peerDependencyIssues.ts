import baseLogger, {
  LogBase,
  Logger,
} from '@pnpm/logger'
import { PeerDependencyIssues } from '@pnpm/types'

export const peerDependencyIssuesLogger = baseLogger('peer-dependency-issues') as Logger<PeerDependencyIssuesMessage>

export interface PeerDependencyIssuesMessage {
  issuesByProjects: PeerDependencyIssues
}

export type PeerDependencyIssuesLog = {name: 'pnpm:peer-dependency-issues'} & LogBase & PeerDependencyIssuesMessage
