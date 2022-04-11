pub use warp_common::cfg_if::cfg_if;
cfg_if! {
    if #[cfg(feature = "indirect")] {
        pub mod indirect;
        pub use indirect::Tesseract;
    } else {
        pub mod direct;
        pub use direct::Tesseract;
    }
}
