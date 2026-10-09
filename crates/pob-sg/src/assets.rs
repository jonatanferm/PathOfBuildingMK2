//! Asset access without `std::fs` (ui doc §3 `assets.rs`, §4.5 "Web"). Owner: U3.
//!
//! SimpleGraphic reads two roots: the script directory (PoB `src/`, where relative image paths
//! resolve) and its own runtime directory (`runtime/SimpleGraphic/`, fonts). Native hosts read
//! the file system, the web host a prefetched VFS, tests an in-memory map.

/// Read access to PoB's two asset roots.
pub trait AssetSource {
    /// A file relative to PoB's `src/` (the script dir), e.g. `"Assets/ring.png"`.
    fn read_src(&self, path: &str) -> std::io::Result<Vec<u8>>;
    /// A file relative to `runtime/SimpleGraphic/`, e.g. `"Fonts/Liberation Sans.tgf"`.
    fn read_runtime(&self, path: &str) -> std::io::Result<Vec<u8>>;
}
