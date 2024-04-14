import { addDistTag } from '@pnpm/registry-mock'

export async function add (packageName: string, version: string, distTag: string): Promise<void> {
  await addDistTag({ package: packageName, version, distTag })
}
