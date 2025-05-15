export interface PrepareExecutionEnvOptions {
  extraBinPaths?: string[]
  executionEnv: ExecutionEnv | undefined
}

export interface PrepareExecutionEnvResult {
  extraBinPaths: string[]
}

export type PrepareExecutionEnv = (options: PrepareExecutionEnvOptions) => Promise<PrepareExecutionEnvResult>

export interface ExecutionEnv {
  nodeVersion?: string
}
