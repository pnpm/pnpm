import { type Project, type ProjectRootDir } from '@pnpm/types'

export type ProjectsList = Array<Pick<Project, 'rootDir'>>

export interface PackagesList {
  workspaceDir: string
  projectRootDirs: ProjectRootDir[]
}
