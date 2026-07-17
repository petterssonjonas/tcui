#[expect(dead_code, reason = "T02 will remove legacy chat-file cache payloads")]
pub mod cache_paths;
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "T08B will integrate ChatSavedFileStore lifecycle")
)]
pub mod chat_files;
pub mod chat_store;
pub mod crypto;
pub mod db;
pub mod paths;

pub use db::Storage;
