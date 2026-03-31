import { PnpmError } from '@pnpm/error'

export interface NormalizedConfigDep {
  version: string
  resolution: {
    integrity: string
    tarball: string
  }
}

export function parseIntegrity (pkgName: string, pkgSpec: string): { version: string, integrity: string } {
  const sepIndex = pkgSpec.indexOf('+')
  if (sepIndex === -1) {
    throw new PnpmError('CONFIG_DEP_NO_INTEGRITY', `Your config dependency called "${pkgName}" doesn't have an integrity checksum`, {
      hint: `Integrity checksum should be inlined in the version specifier. For example:

pnpm-workspace.yaml:
configDependencies:
  my-config: "1.0.0+sha512-Xg0tn4HcfTijTwfDwYlvVCl43V6h4KyVVX2aEm4qdO/PC6L2YvzLHFdmxhoeSA3eslcE6+ZVXHgWwopXYLNq4Q=="
`,
    })
  }
  const version = pkgSpec.substring(0, sepIndex)
  const integrity = pkgSpec.substring(sepIndex + 1)
  return { version, integrity }
}
