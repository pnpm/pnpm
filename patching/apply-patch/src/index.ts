import path from "path";
import { PnpmError } from "@pnpm/error";
import {
  applyPatch,
  getPackageDetailsFromPatchFilename,
} from "patch-package/pnpm";

export interface ApplyPatchToDirOpts {
  patchedDir: string;
  patchFilePath: string;
}

export function applyPatchToDir(opts: ApplyPatchToDirOpts) {
  // Ideally, we would just run "patch" or "git apply".
  // However, "patch" is not available on Windows and "git apply" is hard to execute on a subdirectory of an existing repository
  const cwd = process.cwd();
  process.chdir(opts.patchedDir);

  try {
    applyPatch({
      packageDetails: getPackageDetailsFromPatchFilename(
        path.basename(opts.patchFilePath)
      ),
      patchDir: opts.patchedDir,
      patchFilePath: opts.patchFilePath,
    });
  } catch (err) {
    throw new PnpmError(
      "PATCH_FAILED",
      `Could not apply patch ${opts.patchFilePath} to ${opts.patchedDir}`
    );
  } finally {
    process.chdir(cwd);
  }
}
