import { type PackagesList } from './types'

export const isPackagesList = (value: unknown): value is PackagesList =>
  typeof value === 'object' && value != null && 'workspaceDir' in value && 'projectRootDirs' in value &&
  typeof value.workspaceDir === 'string' && Array.isArray(value.projectRootDirs)
