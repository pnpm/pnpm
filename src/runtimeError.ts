/*
 * An error message with a `silent` flag, where the stack is supressed.
 * Used for user-friendly error messages.
 */

export default function runtimeError (message: string) {
  const err = new Error(message)
  err['silent'] = true
  return err
}
