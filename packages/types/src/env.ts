export type PrepareExecutionEnv = (extraBinPaths: string[], executionEnv: ExecutionEnv | undefined) => Promise<string[]>

export interface ExecutionEnv {
  nodeVersion?: string
}
