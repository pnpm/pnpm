export interface PackageNode {
  alias: string
  circular?: true
  dependencies?: PackageNode[]
  dev?: boolean
  isPeer: boolean
  isSkipped: boolean
  isMissing: boolean
  name: string
  optional?: true
  path: string
  resolved?: string
  searched?: true
  version: string
}
