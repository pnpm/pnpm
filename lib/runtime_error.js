/*
 * An error message with a `silent` flag, where the stack is supressed.
 * Used for user-friendly error messages.
 */

module.exports = function runtimeError (message) {
  var err = new Error(message)
  err.silent = true
  return err
}
