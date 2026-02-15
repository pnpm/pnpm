import { optionsWithOtpEnv } from '../src/otpEnv.js'

describe('optionsWithOtpEnv', () => {
  test('returns the same unchanged options when neither --otp nor PNPM_CONFIG_OTP is defined', () => {
    const input: Record<string, string> = {
      foo: 'hello',
      bar: 'world',
    }
    const expectedOutput = { ...input }
    const actualOutput = optionsWithOtpEnv(input, {})
    expect(actualOutput).toBe(input)
    expect(actualOutput).toStrictEqual(expectedOutput)
  })

  test('returns the same unchanged options when --opt is defined without PNPM_CONFIG_OTP', () => {
    const otp = 'example one-time password'
    const input: Record<string, string> = {
      foo: 'hello',
      bar: 'world',
      otp,
    }
    const expectedOutput = { ...input }
    const actualOutput = optionsWithOtpEnv(input, {})
    expect(actualOutput).toBe(input)
    expect(actualOutput).toStrictEqual(expectedOutput)
    expect(actualOutput.otp).toBe(otp)
  })

  test('returns the same unchanged options when --opt is defined with PNPM_CONFIG_OTP', () => {
    const otp = 'example one-time password'
    const input: Record<string, string> = {
      foo: 'hello',
      bar: 'world',
      otp,
    }
    const expectedOutput = { ...input }
    const PNPM_CONFIG_OTP = 'different one-time password'
    const actualOutput = optionsWithOtpEnv(input, { PNPM_CONFIG_OTP })
    expect(actualOutput).toBe(input)
    expect(actualOutput).toStrictEqual(expectedOutput)
    expect(actualOutput.otp).toBe(otp)
    expect(actualOutput.otp).not.toBe(PNPM_CONFIG_OTP)
  })

  test('returns an options with otp when PNPM_CONFIG_OTP is defined without --otp', () => {
    const input: Record<string, string> = {
      foo: 'hello',
      bar: 'world',
    }
    const PNPM_CONFIG_OTP = 'one-time password from env'
    const expectedOutput = { ...input, otp: PNPM_CONFIG_OTP }
    const actualOutput = optionsWithOtpEnv(input, { PNPM_CONFIG_OTP })
    expect(actualOutput).not.toBe(input)
    expect(actualOutput).toStrictEqual(expectedOutput)
    expect(actualOutput.otp).toBe(PNPM_CONFIG_OTP)
  })
})
