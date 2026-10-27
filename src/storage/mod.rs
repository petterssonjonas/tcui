#[expect(dead_code, reason = "T02 will remove legacy chat-file cache payloads")]
pub mod cache_paths;
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "T08B will integrate ChatSavedFileStore lifecycle")
)]
pub mod chat_files;
pub mod chat_store;
pub(crate) mod conversation_recall;
pub mod crypto;
pub mod db;
pub(crate) mod native_transfer;
pub mod paths;
pub(crate) mod saved_file_migration;

pub use db::Storage;
