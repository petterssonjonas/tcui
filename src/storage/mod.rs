pub mod cache_paths;
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "T08B will integrate ChatFileStore lifecycle")
)]
pub mod chat_files;
pub mod chat_store;
pub mod crypto;
pub mod db;
pub mod paths;

pub use db::Storage;
