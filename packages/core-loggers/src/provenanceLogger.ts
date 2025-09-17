import {
  type LogBase,
  logger,
} from '@pnpm/logger'

export const provenanceLogger = logger<ProvenanceMessage>('provenance')

interface Pkg {
  name: string
  version: string
  provenance: boolean | 'trustedPublisher'
}

export interface ProvenanceMessage {
  pkgs: Pkg[]
}

export type ProvenanceLog = { name: 'pnpm:provenance' } & LogBase & ProvenanceMessage
