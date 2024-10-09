import { type Catalogs } from '@pnpm/catalogs.types'
import { type MANIFEST_BASE_NAMES } from '@pnpm/constants'
import { type Project, type ProjectRootDir } from '@pnpm/types'

export type ProjectsList = Array<Pick<Project, 'rootDir'>>

export type ManifestBaseName = typeof MANIFEST_BASE_NAMES[number]

export interface ProjectInfo {
  manifestBaseName: ManifestBaseName
  manifestModificationTimestamp: number
}

export interface PackagesList {
  catalogs?: Catalogs
  projects: Record<ProjectRootDir, ProjectInfo>
  workspaceDir: string
}
