export function shouldUpdateInjectedFilesAfterRun (scriptName: string, updateInjectedFilesAfterRun: boolean | string[] | undefined): boolean {
  return typeof updateInjectedFilesAfterRun === 'boolean'
    ? updateInjectedFilesAfterRun
    : updateInjectedFilesAfterRun?.includes(scriptName) ?? false;
}
