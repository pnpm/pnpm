export interface PrepareExecutionEnvOptions {
  extraBinPaths?: string[]
}

export interface PrepareExecutionEnvResult {
  extraBinPaths: string[]
}

export type PrepareExecutionEnv = (options: PrepareExecutionEnvOptions) => Promise<PrepareExecutionEnvResult>
