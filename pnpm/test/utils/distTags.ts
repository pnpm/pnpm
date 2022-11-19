import { addDistTag } from '@pnpm/registry-mock'

export async function add (packageName: string, version: string, distTag: string) {
  return addDistTag({ package: packageName, version, distTag })
}
