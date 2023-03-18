import {
  type LogBase,
  type Logger,
  logger,
} from '@pnpm/logger'
import { type PeerDependencyIssuesByProjects } from '@pnpm/types'

export const peerDependencyIssuesLogger = logger('peer-dependency-issues') as Logger<PeerDependencyIssuesMessage>

export interface PeerDependencyIssuesMessage {
  issuesByProjects: PeerDependencyIssuesByProjects
}

export type PeerDependencyIssuesLog = { name: 'pnpm:peer-dependency-issues' } & LogBase & PeerDependencyIssuesMessage
