export function shouldUpdateInjectedPackagesAfterRun (scriptName: string, updateInjectedPackagesAfterRun: boolean | string[] | undefined): boolean {
  return typeof updateInjectedPackagesAfterRun === 'boolean'
    ? updateInjectedPackagesAfterRun
    : updateInjectedPackagesAfterRun?.includes(scriptName) ?? false
}
