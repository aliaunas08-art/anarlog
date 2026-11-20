//! The chain head (`seq:hash` of the latest entry) kept in the Keychain, so
//! cutting entries off the end of `audit.db` is detected (D1.3).
//!
//! Keychain calls can block (a locked login keychain, a permission prompt),
//! so they run on one worker thread and callers wait with a timeout. The app
//! never waits on the Keychain to start or to append.
//!
//! A stored head that is *behind* the database is fine (the last write was
//! lost or is still queued). A head that is *ahead* of it, or names a seq
//! whose hash differs, means entries were removed or rewritten.

use std::sync::Arc;
use std::sync::mpsc;
use std::time::Duration;

use anlg_local_accounts::SecretStore;
use tokio::sync::oneshot;

pub const HEAD_KEY: &str = "audit-chain-head";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Head {
    pub seq: i64,
    pub hash: String,
}

impl Head {
    pub fn encode(&self) -> String {
        format!("{}:{}", self.seq, self.hash)
    }

    pub fn decode(value: &str) -> Option<Self> {
        let (seq, hash) = value.split_once(':')?;
        let seq = seq.parse().ok()?;
        (hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit())).then(|| Head {
            seq,
            hash: hash.to_string(),
        })
    }
}

/// What reading the Keychain returned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeadRead {
    Present(Head),
    /// No item yet (first run).
    Missing,
    /// The Keychain answered with an error or did not answer in time.
    Unavailable(String),
}

enum Command {
    Read(oneshot::Sender<HeadRead>),
    Write(Head),
    Barrier(oneshot::Sender<()>),
}

pub(crate) struct HeadKeeper {
    tx: mpsc::Sender<Command>,
}

impl HeadKeeper {
    pub(crate) fn spawn(store: Arc<dyn SecretStore>) -> Self {
        let (tx, rx) = mpsc::channel::<Command>();
        let spawned = std::thread::Builder::new()
            .name("audit-head".into())
            .spawn(move || worker(store, rx));
        if let Err(error) = spawned {
            tracing::error!(%error, "audit_head_thread_failed");
        }
        Self { tx }
    }

    pub(crate) async fn read(&self, timeout: Duration) -> HeadRead {
        let (reply, rx) = oneshot::channel();
        if self.tx.send(Command::Read(reply)).is_err() {
            return HeadRead::Unavailable("keychain worker stopped".into());
        }
        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(read)) => read,
            Ok(Err(_)) => HeadRead::Unavailable("keychain worker stopped".into()),
            Err(_) => HeadRead::Unavailable("the Keychain did not answer in time".into()),
        }
    }

    /// Queues a write; queued writes are coalesced to the newest.
    pub(crate) fn write(&self, head: Head) {
        let _ = self.tx.send(Command::Write(head));
    }

    /// Waits until every queued command has run (tests and shutdown).
    pub(crate) async fn barrier(&self, timeout: Duration) -> bool {
        let (reply, rx) = oneshot::channel();
        if self.tx.send(Command::Barrier(reply)).is_err() {
            return false;
        }
        matches!(tokio::time::timeout(timeout, rx).await, Ok(Ok(())))
    }
}

fn worker(store: Arc<dyn SecretStore>, rx: mpsc::Receiver<Command>) {
    let flush = |pending: &mut Option<Head>| {
        if let Some(head) = pending.take()
            && let Err(error) = store.set(HEAD_KEY, &head.encode())
        {
            tracing::warn!(error = %error, "audit_head_write_failed");
        }
    };
    while let Ok(command) = rx.recv() {
        match command {
            Command::Write(head) => {
                let mut pending = Some(head);
                // Coalesce whatever else is queued behind it.
                while let Ok(next) = rx.try_recv() {
                    match next {
                        Command::Write(head) => pending = Some(head),
                        Command::Read(reply) => {
                            flush(&mut pending);
                            let _ = reply.send(read_now(store.as_ref()));
                        }
                        Command::Barrier(reply) => {
                            flush(&mut pending);
                            let _ = reply.send(());
                        }
                    }
                }
                flush(&mut pending);
            }
            Command::Read(reply) => {
                let _ = reply.send(read_now(store.as_ref()));
            }
            Command::Barrier(reply) => {
                let _ = reply.send(());
            }
        }
    }
}

fn read_now(store: &dyn SecretStore) -> HeadRead {
    match store.get(HEAD_KEY) {
        // An unreadable value is not "first run": treating it as missing
        // would let a corrupted item reset the anchor.
        Ok(Some(value)) => Head::decode(&value).map_or_else(
            || HeadRead::Unavailable("the stored chain head is unreadable".into()),
            HeadRead::Present,
        ),
        Ok(None) => HeadRead::Missing,
        Err(error) => HeadRead::Unavailable(error.to_string()),
    }
}
