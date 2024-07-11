export type PrepareExecutionEnv = (extraBinPaths: string[], useNodeVersion: string | undefined) => Promise<string[]>
