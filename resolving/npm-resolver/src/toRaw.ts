import { type RegistryPackageSpec } from './parseBareSpecifier'

export function toRaw (spec: RegistryPackageSpec): string {
  return `${spec.name}@${spec.fetchSpec}`
}
