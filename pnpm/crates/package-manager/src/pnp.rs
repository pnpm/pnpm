use crate::package_map::{PackageMapOptions, lockfile_to_package_map};
use derive_more::{Display, Error};
use pacquet_lockfile::Lockfile;
use pacquet_package_manifest::PackageManifest;
use std::path::{Path, PathBuf};

pub const PNP_FILENAME: &str = ".pnp.cjs";

#[derive(Debug, Display, Error)]
pub enum WritePnpFileError {
    #[display("failed to serialize the PnP package registry: {_0}")]
    Serialize(#[error(source)] serde_json::Error),
    #[display("failed to write .pnp.cjs: {_0}")]
    Write(#[error(source)] pacquet_fs::EnsureFileError),
}

pub(crate) fn write_pnp_file(
    lockfile: &Lockfile,
    lockfile_dir: &Path,
    config: &pacquet_config::Config,
    layout: &crate::VirtualStoreLayout,
    project_manifests: &[(PathBuf, &PackageManifest)],
) -> Result<(), WritePnpFileError> {
    let package_map = lockfile_to_package_map(
        lockfile,
        &PackageMapOptions {
            lockfile_dir,
            modules_dir: &config.modules_dir,
            package_map_type: pacquet_config::NodePackageMapType::Standard,
            layout,
            project_manifests,
        },
    );
    let registry = serde_json::to_string(&package_map).map_err(WritePnpFileError::Serialize)?;
    let modules_dir = pathdiff::diff_paths(&config.modules_dir, lockfile_dir)
        .unwrap_or_else(|| config.modules_dir.clone());
    let modules_dir = serde_json::to_string(&modules_dir.to_string_lossy())
        .map_err(WritePnpFileError::Serialize)?;
    let contents = format!(
        r"'use strict';
const Module = require('module');
const path = require('path');
const {{ fileURLToPath }} = require('url');
const registry = {registry}.packages;
const modulesDir = path.resolve(__dirname, {modules_dir});

function packageLocation(pkg) {{
  return pkg.url.startsWith('file:') ? fileURLToPath(pkg.url) : path.resolve(modulesDir, pkg.url);
}}

const locations = Object.entries(registry)
  .map(([id, pkg]) => [id, packageLocation(pkg)])
  .sort((a, b) => b[1].length - a[1].length);
const originalResolveFilename = Module._resolveFilename;

function packageName(request) {{
  if (!request || request[0] === '.' || request[0] === '#' || path.isAbsolute(request)) return null;
  if (request[0] !== '@') return request.split('/', 1)[0];
  const parts = request.split('/');
  return parts.length > 1 ? `${{parts[0]}}/${{parts[1]}}` : request;
}}

function issuerPackage(issuer) {{
  const normalized = path.resolve(issuer || __dirname);
  return locations.find(([, location]) =>
    normalized === location || normalized.startsWith(`${{location}}${{path.sep}}`)
  );
}}

function moduleForIssuer(issuer) {{
  const filename = path.resolve(issuer || __filename);
  const parent = new Module(filename);
  parent.filename = filename;
  parent.paths = Module._nodeModulePaths(path.dirname(filename));
  return parent;
}}

function resolveToUnqualified(request, issuer) {{
  const name = packageName(request);
  if (name === null || request.startsWith('node:') || Module.builtinModules.includes(name)) return null;
  const owner = issuerPackage(issuer);
  if (!owner) return null;
  const dependencyId = registry[owner[0]].dependencies[name];
  if (dependencyId === undefined || registry[dependencyId] === undefined) {{
    const error = new Error(`Your application tried to access ${{name}}, but it isn't declared in your dependencies`);
    error.code = 'MODULE_NOT_FOUND';
    throw error;
  }}
  const subpath = request.slice(name.length);
  const dependencyLocation = packageLocation(registry[dependencyId]);
  const unqualified = path.resolve(dependencyLocation, `.${{subpath}}`);
  if (unqualified !== dependencyLocation && !unqualified.startsWith(`${{dependencyLocation}}${{path.sep}}`)) {{
    const error = new Error(`Your application tried to access a path outside ${{name}}`);
    error.code = 'MODULE_NOT_FOUND';
    throw error;
  }}
  return unqualified;
}}

function resolveRequest(request, issuer, options) {{
  const unqualified = resolveToUnqualified(request, issuer);
  const parent = moduleForIssuer(issuer);
  if (unqualified === null) return originalResolveFilename.call(Module, request, parent, false, options);
  return originalResolveFilename.call(Module, unqualified, parent, false, options);
}}

function setup() {{
  if (Module._resolveFilename.__pnpmPnp) return;
  const hooked = function(request, parent, isMain, options) {{
    const issuer = parent && parent.filename ? parent.filename : __filename;
    const unqualified = resolveToUnqualified(request, issuer);
    if (unqualified === null) return originalResolveFilename.call(this, request, parent, isMain, options);
    return originalResolveFilename.call(this, unqualified, parent, isMain, options);
  }};
  hooked.__pnpmPnp = true;
  Module._resolveFilename = hooked;
  Module.findPnpApi = () => api;
}}

const api = {{ resolveRequest, resolveToUnqualified, setup }};
module.exports = api;
setup();
",
    );
    pacquet_fs::ensure_file(&lockfile_dir.join(PNP_FILENAME), contents.as_bytes(), None)
        .map_err(WritePnpFileError::Write)
}
