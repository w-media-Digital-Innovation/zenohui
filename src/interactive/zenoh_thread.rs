use std::sync::{mpsc, Arc, RwLock, RwLockReadGuard};
use std::thread;

use zenoh::handlers::fifo::FifoChannel;
use zenoh::sample::SampleKind;
use zenoh::{Session, Wait};

use crate::interactive::zenoh_history::ZenohHistory;
use crate::payload::Payload;
use crate::zenoh_client::{HistoryEntry, Time};

type ConnectionErrorArc = Arc<RwLock<Option<String>>>;
type HistoryArc = Arc<RwLock<ZenohHistory>>;

pub struct ZenohThread {
    session: Arc<Session>,
    connection_err: ConnectionErrorArc,
    history: HistoryArc,
}

impl ZenohThread {
    pub fn new(
        session: Arc<Session>,
        subscribe_keyexpr: Vec<String>,
        payload_size_limit: usize,
    ) -> anyhow::Result<Self> {
        let connection_err = Arc::new(RwLock::new(None));
        let history = Arc::new(RwLock::new(ZenohHistory::new()));
        let (tx, rx) = mpsc::channel();

        for keyexpr in &subscribe_keyexpr {
            let session = Arc::clone(&session);
            let keyexpr = keyexpr.clone();
            let tx = tx.clone();
            let connection_err = Arc::clone(&connection_err);
            thread::Builder::new()
                .name(format!("zenoh subscriber {keyexpr}"))
                .spawn(move || {
                    let subscriber = match session
                        .declare_subscriber(&keyexpr)
                        .with(FifoChannel::default())
                        .wait()
                    {
                        Ok(subscriber) => subscriber,
                        Err(err) => {
                            *connection_err.write().unwrap() = Some(err.to_string());
                            return;
                        }
                    };
                    loop {
                        match subscriber.recv() {
                            Ok(sample) => {
                                if tx.send(sample).is_err() {
                                    break;
                                }
                            }
                            Err(err) => {
                                *connection_err.write().unwrap() = Some(err.to_string());
                                break;
                            }
                        }
                    }
                })
                .expect("should be able to spawn a thread");
        }

        drop(tx);

        {
            let history = Arc::clone(&history);
            let connection_err = Arc::clone(&connection_err);
            thread::Builder::new()
                .name("zenoh history".to_owned())
                .spawn(move || {
                    for sample in rx {
                        *connection_err.write().unwrap() = None;
                        let payload = sample.payload().to_bytes().to_vec();
                        let time = if sample.kind() == SampleKind::Delete {
                            Time::Unknown
                        } else {
                            Time::new_now()
                        };
                        history.write().unwrap().add(
                            sample.key_expr().as_str().to_owned(),
                            HistoryEntry {
                                kind: sample.kind(),
                                time,
                                payload_size: payload.len(),
                                payload: Payload::truncated(payload, payload_size_limit),
                            },
                        );
                    }
                })
                .expect("should be able to spawn a thread");
        }

        Ok(Self {
            session,
            connection_err,
            history,
        })
    }

    pub fn has_connection_err(&self) -> Option<String> {
        self.connection_err
            .read()
            .expect("zenoh history thread panicked")
            .as_ref()
            .map(ToString::to_string)
    }

    pub fn get_history(&self) -> RwLockReadGuard<'_, ZenohHistory> {
        self.history
            .read()
            .expect("zenoh history thread panicked")
    }

    /// Remove from local cache
    pub fn uncache_topic_entry(&self, keyexpr: &str, index: usize) -> Option<HistoryEntry> {
        self.history
            .write()
            .expect("zenoh history thread panicked")
            .uncache_topic_entry(keyexpr, index)
    }

    /// Clean on Zenoh
    pub fn clean_below(&self, keyexpr: &str) -> anyhow::Result<()> {
        let keyexprs = self.get_history().get_topics_below(keyexpr);
        for keyexpr in keyexprs {
            self.session
                .put(&keyexpr, Vec::<u8>::new())
                .wait()
                .map_err(|err| anyhow::anyhow!(err))?;
            self.session
                .delete(&keyexpr)
                .wait()
                .map_err(|err| anyhow::anyhow!(err))?;
        }
        Ok(())
    }
}
