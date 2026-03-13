const ENV_KEY = 'PNPM_CONFIG_OTP'

type EnvBase =
& Partial<Readonly<Record<string, string>>>
& Partial<Readonly<Record<typeof ENV_KEY, string>>>

interface OptionsBase {
  readonly otp?: string
}

export const optionsWithOtpEnv = <Options extends OptionsBase> (
  opts: Options,
  { [ENV_KEY]: otp }: EnvBase
): Options =>
  Boolean(opts.otp) || !otp // empty string is considered "not defined" here
    ? opts
    : { ...opts, otp }
