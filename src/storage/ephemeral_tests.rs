use super::*;
use crate::storage::conversation_recall;
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

struct Fixture {
    storage: Storage,
    paths: TcuiDataPaths,
    key: SharedKey,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "tcui-ephemeral-storage-{}-{:032x}",
            std::process::id(),
            rand::random::<u128>()
        ));
        let paths = TcuiDataPaths::from_root(root);
        let key = SharedKey::load_or_create_default(&paths)
            .expect("create storage key")
            .key;
        let storage = Self::open(&paths, &key);
        Self {
            storage,
            paths,
            key,
        }
    }

    fn open(paths: &TcuiDataPaths, key: &SharedKey) -> Storage {
        Storage::from_parts(
            Connection::open_in_memory().expect("open database"),
            paths.clone(),
            key.clone(),
            false,
        )
        .expect("open storage")
    }

    fn saved_conversation(&self) -> i64 {
        let id = self
            .storage
            .create_conversation(3)
            .expect("create saved chat");
        self.storage
            .update_conversation_title(id, "Saved chat")
            .expect("name saved chat");
        self.storage
            .save_message(&message(id, "durable source context"))
            .expect("save source message");
        id
    }

    fn ephemeral_conversation(&self, source: i64) -> i64 {
        let messages = self.storage.get_messages(source).expect("read source");
        self.storage
            .create_ephemeral_conversation(3, source, "Ephemeral fork", &messages)
            .expect("create ephemeral fork")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.paths.root);
    }
}

fn message(conversation_id: i64, content: &str) -> Message {
    Message::new(conversation_id, "user".to_owned(), content.to_owned())
}

fn disk_files(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut files = BTreeMap::new();
    for entry in std::fs::read_dir(root).expect("read storage directory") {
        let path = entry.expect("read directory entry").path();
        if path.is_dir() {
            files.extend(disk_files(&path));
        } else {
            files.insert(path.clone(), std::fs::read(path).expect("read stored file"));
        }
    }
    files
}

#[test]
fn ephemeral_forks_copy_context_and_lineage_without_reusing_conversation_ids() {
    let fixture = Fixture::new();
    let source = fixture.saved_conversation();
    let mut first = message(source, "first context");
    first.id = Some(7);
    first.thinking_content = Some("retained thinking".to_owned());
    first.tool_calls = Some("retained tool calls".to_owned());
    let mut second = message(source, "second context");
    second.id = Some(29);
    second.images = Some("retained image metadata".to_owned());
    second.token_count = Some(42);
    let messages = vec![first, second];
    fixture
        .storage
        .replace_messages(source, &messages)
        .expect("seed source");
    let fork = fixture.ephemeral_conversation(source);
    let other_fork = fixture.ephemeral_conversation(source);
    let document = fixture.storage.read_chat_document(fork).expect("read fork");

    assert!(source > 0);
    assert!(fork < 0 && other_fork < 0);
    assert_ne!(fork, other_fork);
    assert_eq!(document.id, fork);
    assert_eq!(document.tab_id, 3);
    assert_eq!(document.title, "Ephemeral fork");
    let lineage = document.parent.expect("fork lineage");
    assert_eq!(lineage.conversation_id, source);
    assert_eq!(lineage.last_message_id, Some(29));
    assert_eq!(lineage.copied_message_count, 2);
    let mut expected = messages.clone();
    for (index, copied) in expected.iter_mut().enumerate() {
        copied.id = Some(index as i64 + 1);
        copied.conversation_id = fork;
    }
    assert_eq!(
        serde_json::to_value(&document.messages).expect("serialize copied context"),
        serde_json::to_value(&expected).expect("serialize expected context")
    );
    assert_eq!(messages[0].id, Some(7));
    assert_eq!(messages[1].conversation_id, source);
    assert_eq!(
        fixture
            .storage
            .get_messages(source)
            .expect("source messages")[1]
            .id,
        Some(29)
    );
}

#[test]
fn ephemeral_mutations_are_shared_by_existing_clones_and_leave_saved_chats_unchanged() {
    let fixture = Fixture::new();
    let source = fixture.saved_conversation();
    let reader = fixture.storage.chat_reader();
    let saved_files = disk_files(&fixture.paths.root);
    let fork = fixture.ephemeral_conversation(source);
    let other_fork = fixture.ephemeral_conversation(source);

    assert!(reader.conversation_exists(fork).expect("fork exists"));
    assert_eq!(
        reader
            .save_message(&message(fork, "private addition"))
            .expect("append fork"),
        2
    );
    assert_eq!(
        fixture
            .storage
            .get_messages(fork)
            .expect("read appended fork")
            .len(),
        2
    );
    let mut replacement = message(source, "replacement context");
    replacement.id = Some(8);
    fixture
        .storage
        .replace_messages(fork, &[replacement, message(source, "new context")])
        .expect("replace fork messages");
    reader
        .update_conversation_title(fork, "Private renamed")
        .expect("rename fork");
    fixture
        .storage
        .set_conversation_pinned(fork, true)
        .expect("pin fork");
    let document = reader.read_document(fork).expect("read mutated fork");

    assert_eq!(document.title, "Private renamed");
    assert!(document.pinned);
    assert_eq!(document.messages[0].id, Some(8));
    assert_eq!(document.messages[1].id, Some(9));
    assert!(document
        .messages
        .iter()
        .all(|message| message.conversation_id == fork));
    assert_eq!(
        reader.get_messages(other_fork).expect("other fork").len(),
        1
    );
    assert_eq!(
        fixture.storage.get_messages(source).expect("saved context")[0].content,
        "durable source context"
    );
    assert_eq!(disk_files(&fixture.paths.root), saved_files);
}

#[test]
fn ephemeral_chats_never_reach_disk_trash_persistent_listings_export_or_automatic_recall() {
    let fixture = Fixture::new();
    let source = fixture.saved_conversation();
    let saved_files = disk_files(&fixture.paths.root);
    let fork = fixture.ephemeral_conversation(source);
    fixture
        .storage
        .save_message(&message(fork, "saffron lantern private evidence"))
        .expect("append secret");
    fixture
        .storage
        .update_conversation_title(fork, "saffron lantern private title")
        .expect("rename secret");

    let listed = fixture
        .storage
        .get_conversations(3)
        .expect("list saved chats");
    assert_eq!(
        listed.iter().map(|entry| entry.id).collect::<Vec<_>>(),
        [source]
    );
    let (listed, warnings) = fixture
        .storage
        .get_conversations_with_warnings(3)
        .expect("list with warnings");
    assert_eq!(
        listed.iter().map(|entry| entry.id).collect::<Vec<_>>(),
        [source]
    );
    assert_eq!(warnings, 0);
    let exported = fixture
        .storage
        .list_all_chat_documents()
        .expect("list all export documents");
    assert_eq!(
        exported
            .iter()
            .map(|document| document.id)
            .collect::<Vec<_>>(),
        [source]
    );
    let recalled = conversation_recall::recall(
        &fixture.storage.chat_reader(),
        source,
        "saffron lantern",
        &HashSet::new(),
    )
    .expect("recall durable chats");
    assert!(recalled.context.is_empty());
    assert_eq!(disk_files(&fixture.paths.root), saved_files);
    fixture
        .storage
        .delete_conversation(fork)
        .expect("close fork");
    assert_eq!(disk_files(&fixture.paths.root), saved_files);
    assert!(std::fs::read_dir(&fixture.paths.chats_trash_dir)
        .expect("read trash")
        .next()
        .is_none());
}

#[test]
fn deleting_ephemeral_chat_invalidates_clones_and_late_mutations_without_disk_fallback() {
    let fixture = Fixture::new();
    let source = fixture.saved_conversation();
    let fork = fixture.ephemeral_conversation(source);
    let reader = fixture.storage.chat_reader();
    let stale = reader.get_messages(fork).expect("capture stale context");
    let saved_files = disk_files(&fixture.paths.root);
    reader
        .delete_conversation(fork)
        .expect("delete through clone");

    assert!(!fixture
        .storage
        .conversation_exists(fork)
        .expect("deleted fork missing"));
    assert!(fixture.storage.read_chat_document(fork).is_err());
    assert!(reader.get_messages(fork).is_err());
    assert!(reader
        .save_message(&message(fork, "late response"))
        .is_err());
    assert!(fixture.storage.replace_messages(fork, &stale).is_err());
    assert!(reader
        .update_conversation_title(fork, "late title")
        .is_err());
    assert!(reader.set_conversation_pinned(fork, true).is_err());
    assert!(fixture.storage.delete_conversation(fork).is_err());
    assert_eq!(disk_files(&fixture.paths.root), saved_files);
}

#[test]
fn reopening_storage_cannot_read_ephemeral_chats_or_reuse_their_ids() {
    let fixture = Fixture::new();
    let source = fixture.saved_conversation();
    let fork = fixture.ephemeral_conversation(source);
    let reopened = Fixture::open(&fixture.paths, &fixture.key);

    assert!(!reopened
        .conversation_exists(fork)
        .expect("ephemeral chat missing after reopen"));
    assert!(reopened.get_messages(fork).is_err());
    assert!(reopened
        .save_message(&message(fork, "stale reopened write"))
        .is_err());
    assert!(reopened.replace_messages(fork, &[]).is_err());
    let other = reopened
        .create_ephemeral_conversation(3, source, "Another ephemeral fork", &[])
        .expect("create reopened fork");
    assert_ne!(other, fork);
    assert!(fixture
        .storage
        .conversation_exists(fork)
        .expect("original fork remains"));
    assert!(!fixture
        .storage
        .conversation_exists(other)
        .expect("independent store has no reopened fork"));
    let saved = reopened
        .get_messages(source)
        .expect("reopen saved messages");
    assert_eq!(saved[0].content, "durable source context");
    let path = fixture
        .paths
        .chats_dir
        .join(format!("{source:016x}.tcui-chat"));
    let encrypted = std::fs::read_to_string(path).expect("read saved ciphertext");
    assert!(encrypted.starts_with("enc:v1:"));
    assert!(!encrypted.contains("durable source context"));
    reopened
        .delete_conversation(source)
        .expect("delete saved chat");
    assert!(fixture
        .paths
        .chats_trash_dir
        .join(format!("{source:016x}.tcui-chat"))
        .exists());
}

#[test]
fn ephemeral_storage_does_not_hide_saved_chat_decryption_errors() {
    let fixture = Fixture::new();
    let source = fixture.saved_conversation();
    let fork = fixture.ephemeral_conversation(source);
    let path = fixture
        .paths
        .chats_dir
        .join(format!("{source:016x}.tcui-chat"));
    std::fs::write(path, "invalid ciphertext").expect("corrupt saved chat");

    assert!(fixture.storage.conversation_exists(source).is_err());
    assert!(fixture.storage.get_messages(source).is_err());
    assert!(fixture
        .storage
        .save_message(&message(source, "must not replace corrupt ciphertext"))
        .is_err());
    assert!(fixture
        .storage
        .conversation_exists(fork)
        .expect("ephemeral chat remains readable"));
}
