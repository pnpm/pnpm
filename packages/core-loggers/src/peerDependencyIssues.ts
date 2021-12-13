import baseLogger, {
  LogBase,
  Logger,
} from '@pnpm/logger'
import { PeerDependencyIssuesByProjects } from '@pnpm/types'

export const peerDependencyIssuesLogger = baseLogger('peer-dependency-issues') as Logger<PeerDependencyIssuesMessage>

export interface PeerDependencyIssuesMessage {
  issuesByProjects: PeerDependencyIssuesByProjects
}

export type PeerDependencyIssuesLog = {name: 'pnpm:peer-dependency-issues'} & LogBase & PeerDependencyIssuesMessage
