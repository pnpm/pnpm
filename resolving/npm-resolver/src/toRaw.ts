import type { RegistryPackageSpec } from '@pnpm/types'

export function toRaw(spec: RegistryPackageSpec): string {
  return `${spec.name}@${spec.fetchSpec}`
}
