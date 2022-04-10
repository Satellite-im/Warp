use warp_common::cfg_if::cfg_if;

cfg_if! {
    if #[cfg(feature = "constellation")] {
        pub use warp_constellation::{
            directory, file, item,
            constellation::{Constellation, ConstellationDataType, ConstellationVersion},
        };
        pub use warp_fs_memory::MemorySystem;
        pub use warp_fs_storj::{StorjClient, StorjFilesystem};
        pub use warp_fs_ipfs::IpfsFileSystem;
    }
}
cfg_if! {
    if #[cfg(feature = "pocket-dimension")] {
        pub use warp_pocket_dimension::{
            query::{Comparator, QueryBuilder},
            DimensionData, PocketDimension,
        };
        pub use warp_pd_stretto::StrettoClient;
        pub use warp_pd_flatfile::FlatfileStorage;
    }
}

cfg_if! {
    if #[cfg(feature = "multipass")] {
        pub use warp_multipass::{identity::*, MultiPass};
    }
}

cfg_if! {
    if #[cfg(feature = "raygun")] {
        pub use warp_raygun::{self, RayGun};
    }
}
