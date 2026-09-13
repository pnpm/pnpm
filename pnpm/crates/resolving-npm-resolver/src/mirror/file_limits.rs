/// One-time, best-effort raise of the process's soft `RLIMIT_NOFILE`
/// toward the hard limit. Loaded mirrors keep their file handle open
/// so version fragments can be read on demand without buffering the
/// body (see [`super::load_meta`]), which holds one descriptor per packument
/// — beyond the conservative soft defaults some platforms ship (256
/// on macOS, 1024 on several Linux distros) once a workspace consults
/// thousands of packuments. Raising the soft limit to the hard limit
/// needs no privileges; it is the same startup adjustment the Go
/// runtime performs.
#[cfg(unix)]
pub(super) fn raise_open_file_limit_once() {
    static RAISE: std::sync::Once = std::sync::Once::new();
    RAISE.call_once(|| {
        // SAFETY: plain libc calls; `limit` is a properly initialised
        // out-parameter and no pointer outlives its call.
        unsafe {
            let mut limit = libc::rlimit {
                rlim_cur: 0,
                rlim_max: 0,
            };
            if libc::getrlimit(libc::RLIMIT_NOFILE, &raw mut limit) != 0 {
                return;
            }
            let ceiling: libc::rlim_t = 1 << 20;
            let target = limit.rlim_max.min(ceiling);
            if target <= limit.rlim_cur {
                return;
            }
            let request = libc::rlimit {
                rlim_cur: target,
                rlim_max: limit.rlim_max,
            };
            if libc::setrlimit(libc::RLIMIT_NOFILE, &raw const request) != 0 {
                // macOS rejects soft limits above `kern.maxfilesperproc`
                // even when the hard limit reads unlimited; 10240 is
                // the historically safe `OPEN_MAX` ceiling there.
                #[cfg(target_os = "macos")]
                {
                    let fallback = limit.rlim_max.min(10240);
                    if fallback > limit.rlim_cur {
                        let request = libc::rlimit {
                            rlim_cur: fallback,
                            rlim_max: limit.rlim_max,
                        };
                        let _ = libc::setrlimit(libc::RLIMIT_NOFILE, &raw const request);
                    }
                }
            }
        }
    });
}

/// Windows has no `RLIMIT_NOFILE`; per-process handle capacity is far
/// above any realistic packument count.
#[cfg(not(unix))]
pub(super) fn raise_open_file_limit_once() {}
