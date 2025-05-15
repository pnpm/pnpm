import {
  type LogBase,
  logger,
} from '@pnpm/logger'
import { type ProjectManifest } from '@pnpm/types'

export const packageManifestLogger = logger<PackageManifestMessage>('package-manifest')

export interface PackageManifestMessageBase {
  prefix: string
  initial?: ProjectManifest
  updated?: ProjectManifest
}

export interface PackageManifestMessageInitial extends PackageManifestMessageBase {
  initial: ProjectManifest
}

export interface PackageManifestMessageUpdated extends PackageManifestMessageBase {
  updated: ProjectManifest
}

export type PackageManifestMessage = PackageManifestMessageInitial | PackageManifestMessageUpdated

export type PackageManifestLog = { name: 'pnpm:package-manifest' } & LogBase & PackageManifestMessage
