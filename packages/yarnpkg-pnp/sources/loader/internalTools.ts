import {Path, npath} from '@yarnpkg/fslib';

export enum ErrorCode {
  API_ERROR = `API_ERROR`,
  BLACKLISTED = `BLACKLISTED`,
  BUILTIN_NODE_RESOLUTION_FAILED = `BUILTIN_NODE_RESOLUTION_FAILED`,
  MISSING_DEPENDENCY = `MISSING_DEPENDENCY`,
  MISSING_PEER_DEPENDENCY = `MISSING_PEER_DEPENDENCY`,
  QUALIFIED_PATH_RESOLUTION_FAILED = `QUALIFIED_PATH_RESOLUTION_FAILED`,
  INTERNAL = `INTERNAL`,
  UNDECLARED_DEPENDENCY = `UNDECLARED_DEPENDENCY`,
  UNSUPPORTED = `UNSUPPORTED`,
}

// Some errors are exposed as MODULE_NOT_FOUND for compatibility with packages
// that expect this umbrella error when the resolution fails
const MODULE_NOT_FOUND_ERRORS = new Set([
  ErrorCode.BLACKLISTED,
  ErrorCode.BUILTIN_NODE_RESOLUTION_FAILED,
  ErrorCode.MISSING_DEPENDENCY,
  ErrorCode.MISSING_PEER_DEPENDENCY,
  ErrorCode.QUALIFIED_PATH_RESOLUTION_FAILED,
  ErrorCode.UNDECLARED_DEPENDENCY,
]);

/**
 * Simple helper function that assign an error code to an error, so that it can more easily be caught and used
 * by third-parties.
 */

export function makeError(pnpCode: ErrorCode, message: string, data: Object = {}): Error {
  const code = MODULE_NOT_FOUND_ERRORS.has(pnpCode)
    ? `MODULE_NOT_FOUND`
    : pnpCode;

  const propertySpec = {
    configurable: true,
    writable: true,
    enumerable: false,
  };

  return Object.defineProperties(new Error(message), {
    code: {
      ...propertySpec,
      value: code,
    },
    pnpCode: {
      ...propertySpec,
      value: pnpCode,
    },
    data: {
      ...propertySpec,
      value: data,
    },
  });
}

/**
 * Returns the module that should be used to resolve require calls. It's usually the direct parent, except if we're
 * inside an eval expression.
 */

export function getIssuerModule(parent: NodeModule | null | undefined): NodeModule | null {
  let issuer = parent;

  while (issuer && (issuer.id === `[eval]` || issuer.id === `<repl>` || !issuer.filename))
    issuer = issuer.parent;

  return issuer || null;
}

export function getPathForDisplay(p: Path) {
  return npath.normalize(npath.fromPortablePath(p));
}
