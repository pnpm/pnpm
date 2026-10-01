use std::{fs::File, io, path::Path};

pub(super) struct FileCreation<'parent> {
    #[cfg(not(target_os = "wasi"))]
    options: std::fs::OpenOptions,
    #[cfg(unix)]
    mode: crate::file_mode::UnixCreationMode,
    #[cfg(target_os = "wasi")]
    mode: Option<u32>,
    #[cfg(target_os = "wasi")]
    parent: &'parent Path,
    #[cfg(not(target_os = "wasi"))]
    parent: std::marker::PhantomData<&'parent Path>,
}

impl<'parent> FileCreation<'parent> {
    #[inline]
    pub(super) fn new(parent: &'parent Path, requested: Option<u32>) -> Self {
        #[cfg(not(target_os = "wasi"))]
        let mut options = std::fs::OpenOptions::new();
        #[cfg(not(target_os = "wasi"))]
        options.write(true).create_new(true);
        #[cfg(unix)]
        let mode = crate::file_mode::unix_creation_mode(parent, requested);
        #[cfg(unix)]
        mode.apply_to(&mut options);
        #[cfg(windows)]
        let _ = (parent, requested);
        Self {
            #[cfg(not(target_os = "wasi"))]
            options,
            #[cfg(unix)]
            mode,
            #[cfg(target_os = "wasi")]
            mode: requested,
            #[cfg(target_os = "wasi")]
            parent,
            #[cfg(not(target_os = "wasi"))]
            parent: std::marker::PhantomData,
        }
    }

    #[inline]
    pub(super) fn open(&self, path: &Path) -> io::Result<File> {
        #[cfg(not(target_os = "wasi"))]
        let open = || self.options.open(path);
        #[cfg(target_os = "wasi")]
        let open = || crate::wasi_fs::create_inheriting_mode(self.parent, path, self.mode);
        super::retry_on_fd_pressure(open)
    }

    #[cfg(unix)]
    #[inline]
    pub(super) fn grant(&self, file: &File) -> io::Result<()> {
        self.mode.grant(file)
    }
}
