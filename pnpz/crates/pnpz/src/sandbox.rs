//! Runs a process with a store that lives elsewhere mounted at `/nix`, without
//! root, by giving the process its own user and mount namespaces.
//!
//! The namespaces need a kernel that lets unprivileged users create them.
//! Inside, the user keeps their uid, so files are owned as before, but setuid
//! programs such as `sudo` do not gain privileges.

use derive_more::{
    Display,
    Error,
};
use miette::Diagnostic;
use nix::{
    mount::{
        MntFlags,
        MsFlags,
        mount,
        umount2,
    },
    sched::{
        CloneFlags,
        unshare,
    },
    unistd::{
        chdir,
        getgid,
        getuid,
        pivot_root,
    },
};
use std::{
    env,
    fs::{
        self,
        File,
    },
    io,
    os::unix::fs::symlink,
    path::Path,
};

#[derive(Debug, Display, Error, Diagnostic)]
#[display("failed to mount the store at /nix ({step}): {source}")]
#[diagnostic(
    code(ERR_PNPZ_SANDBOX),
    help(
        "pnpz mounts its store in a user namespace. If this system forbids unprivileged user namespaces, create a /nix/store directory you own and pnpz will use it directly."
    )
)]
pub struct SandboxError {
    step: &'static str,
    source: io::Error,
}

const NO_PATH: Option<&Path> = None;

fn step<Failure: Into<io::Error>>(step: &'static str) -> impl FnOnce(Failure) -> SandboxError {
    move |source| SandboxError { step, source: source.into() }
}

/// Moves the calling process into namespaces where `store_root` is `/nix` and
/// everything else is where it was.
///
/// The process must have a single thread, which the kernel requires for a new
/// user namespace. The change cannot be undone, so the caller is expected to
/// `exec` the program it prepared the store for.
pub fn enter(store_root: &Path) -> Result<(), SandboxError> {
    let working_directory = env::current_dir().ok();
    let new_root = store_root.join("var/pnpz/root");
    fs::create_dir_all(&new_root).map_err(step("creating the mount point"))?;
    unshare_as_current_user()?;
    mount(NO_PATH, "/", NO_PATH, MsFlags::MS_REC | MsFlags::MS_PRIVATE, NO_PATH)
        .map_err(step("making mounts private"))?;
    mount(Some("tmpfs"), &new_root, Some("tmpfs"), MsFlags::empty(), Some("mode=0755"))
        .map_err(step("mounting the new root"))?;
    // The new root sits below a host directory that is about to be mirrored
    // into it. Unbindable, it is left out of that recursive bind.
    mount(NO_PATH, &new_root, NO_PATH, MsFlags::MS_UNBINDABLE, NO_PATH)
        .map_err(step("isolating the new root"))?;
    mirror_host_root(&new_root)?;
    let nix = new_root.join("nix");
    fs::create_dir(&nix).map_err(step("creating /nix"))?;
    bind(store_root, &nix).map_err(step("binding the store"))?;
    switch_root(&new_root)?;
    // Not finding the old working directory is not an error: it can be
    // unreadable to the user, and the command still runs from `/`.
    let _ = working_directory.map(|directory| chdir(&directory));
    Ok(())
}

/// Creates the namespaces and maps the user to their own uid and gid in them.
fn unshare_as_current_user() -> Result<(), SandboxError> {
    let (uid, gid) = (getuid(), getgid());
    unshare(CloneFlags::CLONE_NEWUSER | CloneFlags::CLONE_NEWNS)
        .map_err(step("creating a user namespace"))?;
    fs::write("/proc/self/setgroups", "deny")
        .and_then(|()| fs::write("/proc/self/uid_map", format!("{uid} {uid} 1")))
        .and_then(|()| fs::write("/proc/self/gid_map", format!("{gid} {gid} 1")))
        .map_err(step("mapping the user"))
}

/// Gives `new_root` every entry of `/` except `nix`: symlinks are copied, and
/// directories and files are bound with the mounts below them.
fn mirror_host_root(new_root: &Path) -> Result<(), SandboxError> {
    let entries = fs::read_dir("/").map_err(step("listing /"))?;
    for entry in entries {
        let entry = entry.map_err(step("listing /"))?;
        if entry.file_name() == "nix" {
            continue;
        }
        mirror_entry(&entry.path(), &new_root.join(entry.file_name()))
            .map_err(step("mirroring the host's root directory"))?;
    }
    Ok(())
}

fn mirror_entry(source: &Path, target: &Path) -> io::Result<()> {
    let file_type = fs::symlink_metadata(source)?.file_type();
    if file_type.is_symlink() {
        return symlink(fs::read_link(source)?, target);
    }
    if file_type.is_dir() {
        fs::create_dir(target)?;
    } else {
        File::create(target)?;
    }
    bind(source, target)
}

fn bind(source: &Path, target: &Path) -> io::Result<()> {
    mount(Some(source), target, NO_PATH, MsFlags::MS_BIND | MsFlags::MS_REC, NO_PATH)?;
    Ok(())
}

/// Makes `new_root` the root directory and detaches the old one.
///
/// `pivot_root` is used, not `chroot`, because a process in a chroot cannot
/// create user namespaces, which sandboxed programs inside the environment
/// need.
fn switch_root(new_root: &Path) -> Result<(), SandboxError> {
    chdir(new_root)
        .and_then(|()| pivot_root(".", "."))
        .and_then(|()| umount2(".", MntFlags::MNT_DETACH))
        .and_then(|()| chdir("/"))
        .map_err(step("switching to the new root"))?;
    // Bindable again, so that a nested sandbox can bind `/`.
    mount(NO_PATH, "/", NO_PATH, MsFlags::MS_REC | MsFlags::MS_PRIVATE, NO_PATH)
        .map_err(step("making the new root bindable"))
}
