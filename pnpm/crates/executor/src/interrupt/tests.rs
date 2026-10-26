use super::kernel_gave_it_to_the_child;

/// One row of [`kernel_gave_it_to_the_child`]'s table, named for what it
/// varies from the one case where the kernel has already delivered the
/// signal: a child in pnpm's own process group, interrupted by the
/// terminal pnpm holds, after the child existed.
struct Case {
    what_differs: &'static str,
    missed_the_child: bool,
    target: i32,
    signal: libc::c_int,
    pnpm_holds_the_terminal: bool,
    kernel_delivered_it: bool,
}

/// The relay may skip a child only where the kernel has already signalled
/// it. Every departure from that one case leaves the child unsignalled,
/// and a relay that skips it leaves pnpm waiting for a script nothing
/// asked to stop.
#[test]
fn only_a_child_the_kernel_signalled_is_left_to_it() {
    let cases = [
        Case {
            what_differs: "nothing: the terminal signalled the group the child was in",
            missed_the_child: false,
            target: 4321,
            signal: libc::SIGINT,
            pnpm_holds_the_terminal: true,
            kernel_delivered_it: true,
        },
        Case {
            what_differs: "the child did not exist when pnpm took the signal",
            missed_the_child: true,
            target: 4321,
            signal: libc::SIGINT,
            pnpm_holds_the_terminal: true,
            kernel_delivered_it: false,
        },
        Case {
            what_differs: "the child leads a process group of its own",
            missed_the_child: false,
            target: -4321,
            signal: libc::SIGINT,
            pnpm_holds_the_terminal: true,
            kernel_delivered_it: false,
        },
        Case {
            what_differs: "the signal was a termination, which no terminal sends",
            missed_the_child: false,
            target: 4321,
            signal: libc::SIGTERM,
            pnpm_holds_the_terminal: true,
            kernel_delivered_it: false,
        },
        Case {
            what_differs: "pnpm holds no terminal, so only `kill` reached it",
            missed_the_child: false,
            target: 4321,
            signal: libc::SIGINT,
            pnpm_holds_the_terminal: false,
            kernel_delivered_it: false,
        },
    ];

    for case in cases {
        let delivered = kernel_gave_it_to_the_child(
            case.missed_the_child,
            case.target,
            case.signal,
            case.pnpm_holds_the_terminal,
        );
        assert_eq!(delivered, case.kernel_delivered_it, "{}", case.what_differs);
    }
}
