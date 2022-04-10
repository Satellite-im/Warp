use warp_common::cfg_if::cfg_if;

cfg_if! {
    if #[cfg(feature = "constellation")] {
        pub use warp_constellation as constellation;
        pub use warp_fs_memory as fs_memory;
        pub use warp_fs_storj as fs_storj;
        pub use warp_fs_ipfs as fs_ipfs;
    }
}
cfg_if! {
    if #[cfg(feature = "pocket-dimension")] {
        pub use warp_pocket_dimension as pocket_dimension;
        pub use warp_pd_stretto as pd_stretto;
        pub use warp_pd_flatfile as pd_flatfile;
    }
}

cfg_if! {
    if #[cfg(feature = "multipass")] {
        pub use warp_multipass as multipass;
    }
}

cfg_if! {
    if #[cfg(feature = "raygun")] {
        pub use warp_raygun as raygun;
    }
}
