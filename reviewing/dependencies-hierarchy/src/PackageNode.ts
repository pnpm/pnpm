export interface PackageNode {
  alias: string
  circular?: boolean | undefined
  dependencies?: PackageNode[] | undefined
  dev?: boolean
  isPeer: boolean
  isSkipped: boolean
  isMissing: boolean
  name: string
  optional?: boolean | undefined
  path: string
  resolved?: string | undefined
  searched?: boolean | undefined
  version: string
}
