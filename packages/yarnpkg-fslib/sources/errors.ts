function makeError(code: string, message: string) {
  return Object.assign(new Error(`${code}: ${message}`), {code});
}

export function EBUSY(message: string) {
  return makeError(`EBUSY`, message);
}

export function ENOSYS(message: string, reason: string) {
  return makeError(`ENOSYS`, `${message}, ${reason}`);
}

export function EINVAL(reason: string) {
  return makeError(`EINVAL`, `invalid argument, ${reason}`);
}

export function EBADF(reason: string) {
  return makeError(`EBADF`, `bad file descriptor, ${reason}`);
}

export function ENOENT(reason: string) {
  return makeError(`ENOENT`, `no such file or directory, ${reason}`);
}

export function ENOTDIR(reason: string) {
  return makeError(`ENOTDIR`, `not a directory, ${reason}`);
}

export function EISDIR(reason: string) {
  return makeError(`EISDIR`, `illegal operation on a directory, ${reason}`);
}

export function EEXIST(reason: string) {
  return makeError(`EEXIST`, `file already exists, ${reason}`);
}

export function EROFS(reason: string) {
  return makeError(`EROFS`, `read-only filesystem, ${reason}`);
}

export function ENOTEMPTY(reason: string) {
  return makeError(`ENOTEMPTY`, `directory not empty, ${reason}`);
}

export function EOPNOTSUPP(reason: string) {
  return makeError(`EOPNOTSUPP`, `operation not supported, ${reason}`);
}

// ------------------------------------------------------------------------

export function ERR_DIR_CLOSED() {
  return makeError(`ERR_DIR_CLOSED`, `Directory handle was closed`);
}

// ------------------------------------------------------------------------

export class LibzipError extends Error {
  code: string;

  constructor(message: string, code: string) {
    super(message);
    this.name = `Libzip Error`;
    this.code = code;
  }
}
