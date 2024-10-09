import { type Project, type ProjectRootDir } from '@pnpm/types'

export type ProjectsList = Array<Pick<Project, 'rootDir'>>

export interface TimestampMap {
  'package.json': number
}

export interface PackagesList {
  workspaceDir: string
  modificationTimestamps: Record<ProjectRootDir, TimestampMap>
}
