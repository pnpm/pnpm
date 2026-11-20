export async function exit (status: number): Promise<never> {
  if (process.platform === 'win32') {
    try {
      // Work around https://github.com/nodejs/node/issues/56645.
      const { destroyDispatchers } = await import('@pnpm/network.fetch')
      await destroyDispatchers()
    } catch {
      // ignore error here
    }
  }
  // eslint-disable-next-line n/no-process-exit -- callers rely on the process ending here with this status
  process.exit(status)
}
