#![allow(unused_macros)]

macro_rules! cfg_core {
    ($($item:item)*) => {
        $(
            #[cfg(feature = "core")]
            #[cfg_attr(docsrs, doc(cfg(feature = "core")))]
            $item
        )*
    }
}

macro_rules! cfg_replication_or_remote_or_sync {
    ($($item:item)*) => {
        $(
            #[cfg(any(feature = "replication", feature = "sync", feature = "remote"))]
            #[cfg_attr(docsrs, doc(cfg(any(feature = "replication", feature = "sync", feature = "remote"))))]
            $item
        )*
    }
}

macro_rules! cfg_replication_or_remote_or_hrana {
    ($($item:item)*) => {
        $(
            #[cfg(any(feature = "replication", feature = "remote", feature = "hrana"))]
            #[cfg_attr(docsrs, doc(cfg(any(feature = "replication", feature = "remote", feature = "hrana"))))]
            $item
        )*
    }
}

macro_rules! cfg_replication {
    ($($item:item)*) => {
        $(
            #[cfg(feature = "replication")]
            #[cfg_attr(docsrs, doc(cfg(feature = "replication")))]
            $item
        )*
    }
}

macro_rules! cfg_sync {
    ($($item:item)*) => {
        $(
            #[cfg(feature = "sync")]
            #[cfg_attr(docsrs, doc(cfg(feature = "sync")))]
            $item
        )*
    }
}

macro_rules! cfg_replication_or_sync {
    ($($item:item)*) => {
        $(
            #[cfg(any(feature = "replication", feature = "sync"))]
            #[cfg_attr(docsrs, doc(cfg(any(feature = "replication", feature = "sync"))))]
            $item
        )*
    }
}

macro_rules! cfg_parser {
    ($($item:item)*) => {
        $(
            #[cfg(feature = "parser")]
            #[cfg_attr(docsrs, doc(cfg(feature = "parser")))]
            $item
        )*
    }
}

macro_rules! cfg_hrana {
    ($($item:item)*) => {
        $(
            #[cfg(feature = "hrana")]
            #[cfg_attr(docsrs, doc(cfg(feature = "hrana")))]
            $item
        )*
    }
}

macro_rules! cfg_remote {
    ($($item:item)*) => {
        $(
            #[cfg(feature = "remote")]
            #[cfg_attr(docsrs, doc(cfg(feature = "remote")))]
            $item
        )*
    }
}

macro_rules! cfg_cloudflare {
    ($($item:item)*) => {
        $(
            #[cfg(feature = "cloudflare")]
            #[cfg_attr(docsrs, doc(cfg(feature = "cloudflare")))]
            $item
        )*
    }
}

macro_rules! cfg_wasm {
    ($($item:item)*) => {
        $(
            #[cfg(feature = "wasm")]
            #[cfg_attr(docsrs, doc(cfg(feature = "wasm")))]
            $item
        )*
    }
}
