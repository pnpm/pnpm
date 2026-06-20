import util from 'node:util'

export function getRegistryConfigKey (registryUrl: string): string {
  const url = new URL(registryUrl)
  return `//${url.host}${url.pathname}`
}

export async function safeReadIniFile (
  readIniFile: (configPath: string) => Promise<object>,
  configPath: string
): Promise<object> {
  try {
    return await readIniFile(configPath)
  } catch (err: unknown) {
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') return {}
    throw err
  }
}
