import fs from "fs";
import path from "path";
import { tempDir } from "@pnpm/prepare";
import type { SymlinkAllModulesMessage } from "../src/types.js";

const originalModule = await import("../src/start.js");
const { symlinkAllModules } = originalModule;

test("symlinkAllModules handles EEXIST when symlink already exists with same target", () => {
  const tmp = tempDir(false);
  const modulesDir = path.join(tmp, "node_modules");
  const pkgDir1 = path.join(tmp, "pkg1");
  const pkgDir2 = path.join(tmp, "pkg2");

  fs.mkdirSync(modulesDir, { recursive: true });
  fs.mkdirSync(pkgDir1, { recursive: true });
  fs.mkdirSync(pkgDir2, { recursive: true });

  fs.writeFileSync(path.join(pkgDir1, "index.js"), "module.exports = {}");
  fs.writeFileSync(path.join(pkgDir2, "index.js"), "module.exports = {}");

  const message: SymlinkAllModulesMessage = {
    type: "symlinkAllModules",
    deps: [
      {
        name: "parent",
        modules: modulesDir,
        children: {
          pkg1: pkgDir1,
          pkg2: pkgDir2,
        },
      },
    ],
  };

  // First call should succeed
  symlinkAllModules(message);

  // Second call should also succeed (EEXIST should be handled)
  symlinkAllModules(message);

  // Verify symlinks are correct
  expect(fs.existsSync(path.join(modulesDir, "pkg1/index.js"))).toBe(true);
  expect(fs.existsSync(path.join(modulesDir, "pkg2/index.js"))).toBe(true);

  const pkg1Link = fs.readlinkSync(path.join(modulesDir, "pkg1"));
  const pkg2Link = fs.readlinkSync(path.join(modulesDir, "pkg2"));

  expect(path.resolve(modulesDir, pkg1Link)).toBe(path.resolve(pkgDir1));
  expect(path.resolve(modulesDir, pkg2Link)).toBe(path.resolve(pkgDir2));
});

test("symlinkAllModules handles EEXIST when creating symlink concurrently", () => {
  const tmp = tempDir(false);
  const modulesDir = path.join(tmp, "node_modules");
  const pkgDir = path.join(tmp, "pkg");

  fs.mkdirSync(modulesDir, { recursive: true });
  fs.mkdirSync(pkgDir, { recursive: true });
  fs.writeFileSync(path.join(pkgDir, "index.js"), "module.exports = {}");

  // Create symlink first to simulate concurrent creation by another worker
  fs.symlinkSync(pkgDir, path.join(modulesDir, "pkg"), "dir");

  const message: SymlinkAllModulesMessage = {
    type: "symlinkAllModules",
    deps: [
      {
        name: "parent",
        modules: modulesDir,
        children: {
          pkg: pkgDir,
        },
      },
    ],
  };

  // Should not throw EEXIST error even when symlink already exists
  expect(() => {
    symlinkAllModules(message);
  }).not.toThrow();

  // Verify symlink still points to correct target
  const linkTarget = fs.readlinkSync(path.join(modulesDir, "pkg"));
  expect(path.resolve(modulesDir, linkTarget)).toBe(path.resolve(pkgDir));
});
