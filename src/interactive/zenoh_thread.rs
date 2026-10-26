use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock, RwLockReadGuard, mpsc};
use std::thread;
use std::time::Duration;

use zenoh::config::WhatAmI;
use zenoh::handlers::fifo::FifoChannel;
use zenoh::sample::SampleKind;
use zenoh::{Config, Session, Wait};

use crate::interactive::scouting::ScoutCache;
use crate::interactive::zenoh_history::ZenohHistory;
use crate::payload::Payload;
use crate::zenoh_client::{HistoryEntry, Time};

type ConnectionErrorArc = Arc<RwLock<Option<String>>>;
type HistoryArc = Arc<RwLock<ZenohHistory>>;
type ScoutArc = Arc<RwLock<ScoutCache>>;

fn limited_payload(bytes: &zenoh::bytes::ZBytes, limit: usize) -> std::io::Result<Payload> {
    let mut data = Vec::new();
    bytes
        .reader()
        .take(u64::try_from(limit.saturating_add(1)).unwrap_or(u64::MAX))
        .read_to_end(&mut data)?;
    Ok(Payload::truncated(data, limit))
}

pub struct ZenohThread {
    workers: Vec<thread::JoinHandle<()>>,
    stop: Arc<AtomicBool>,
    session: Arc<Session>,
    connection_err: ConnectionErrorArc,
    history: HistoryArc,
    scouting: ScoutArc,
}

impl ZenohThread {
    #[allow(clippy::too_many_lines)]
    pub fn new(
        session: Arc<Session>,
        subscribe_keyexpr: &[String],
        payload_size_limit: usize,
        history_limit: usize,
        scout_config: Config,
    ) -> anyhow::Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let connection_err = Arc::new(RwLock::new(None));
        let history = Arc::new(RwLock::new(ZenohHistory::with_limit(history_limit)));
        let scouting = Arc::new(RwLock::new(ScoutCache::default()));
        let (tx, rx) = mpsc::sync_channel(32);

        let mut workers = Vec::new();
        let subscribers = subscribe_keyexpr
            .iter()
            .map(|keyexpr| {
                let subscriber = session
                    .declare_subscriber(keyexpr.as_str())
                    .with(FifoChannel::new(16))
                    .wait()
                    .map_err(|err| anyhow::anyhow!("Subscription failed: {err}"))?;
                Ok((keyexpr.clone(), subscriber))
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        for (keyexpr, subscriber) in subscribers {
            let stop = Arc::clone(&stop);
            let tx = tx.clone();
            let connection_err = Arc::clone(&connection_err);
            workers.push(
                thread::Builder::new()
                    .name(format!("zenoh subscriber {keyexpr}"))
                    .spawn(move || {
                        while !stop.load(Ordering::Relaxed) {
                            match subscriber.recv_timeout(Duration::from_millis(250)) {
                                Ok(Some(sample)) => {
                                    let size = sample.payload().len();
                                    let Ok(payload) =
                                        limited_payload(sample.payload(), payload_size_limit)
                                    else {
                                        continue;
                                    };
                                    let entry = HistoryEntry {
                                        kind: sample.kind(),
                                        time: if sample.kind() == SampleKind::Delete {
                                            Time::Unknown
                                        } else {
                                            Time::new_now()
                                        },
                                        payload_size: size,
                                        payload,
                                    };
                                    let mut pending =
                                        (sample.key_expr().as_str().to_owned(), entry);
                                    drop(sample);
                                    loop {
                                        match tx.try_send(pending) {
                                            Ok(()) => break,
                                            Err(mpsc::TrySendError::Full(item)) => {
                                                if stop.load(Ordering::Relaxed) {
                                                    return;
                                                }
                                                pending = item;
                                                thread::sleep(Duration::from_millis(5));
                                            }
                                            Err(mpsc::TrySendError::Disconnected(_)) => return,
                                        }
                                    }
                                }
                                Ok(None) => {}
                                Err(err) => {
                                    *connection_err.write().unwrap() = Some(err.to_string());
                                    break;
                                }
                            }
                        }
                    })
                    .expect("should be able to spawn a thread"),
            );
        }

        drop(tx);

        {
            let history = Arc::clone(&history);
            let connection_err = Arc::clone(&connection_err);
            workers.push(
                thread::Builder::new()
                    .name("zenoh history".to_owned())
                    .spawn(move || {
                        for (keyexpr, entry) in rx {
                            *connection_err.write().unwrap() = None;
                            history.write().unwrap().add(keyexpr, entry);
                        }
                    })
                    .expect("should be able to spawn a thread"),
            );
        }

        {
            let scouting = Arc::clone(&scouting);
            let stop = Arc::clone(&stop);
            workers.push(
                thread::Builder::new()
                    .name("zenoh scout".to_owned())
                    .spawn(move || {
                        let scout = match zenoh::scout(
                            WhatAmI::Peer | WhatAmI::Router,
                            scout_config,
                        )
                        .wait()
                        {
                            Ok(scout) => scout,
                            Err(err) => {
                                scouting.write().unwrap().set_error(err.to_string());
                                return;
                            }
                        };

                        while !stop.load(Ordering::Relaxed) {
                            match scout.recv_timeout(Duration::from_millis(250)) {
                                Ok(Some(hello)) => {
                                    scouting.write().unwrap().update(&hello);
                                }
                                Ok(None) => {}
                                Err(err) => {
                                    scouting.write().unwrap().set_error(err.to_string());
                                    break;
                                }
                            }
                        }
                    })
                    .expect("should be able to spawn a thread"),
            );
        }

        Ok(Self {
            workers,
            stop,
            session,
            connection_err,
            history,
            scouting,
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
        self.history.read().expect("zenoh history thread panicked")
    }

    pub fn get_scouting(&self) -> RwLockReadGuard<'_, ScoutCache> {
        self.scouting.write().unwrap().sync_connected(
            self.session
                .info()
                .routers_zid()
                .wait()
                .map(|id| id.to_string())
                .collect(),
            self.session
                .info()
                .peers_zid()
                .wait()
                .map(|id| id.to_string())
                .collect(),
        );
        self.scouting.read().expect("zenoh scout thread panicked")
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

impl Drop for ZenohThread {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = self.session.close().wait();
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use super::*;
    use crate::cli::{SessionMode, ZenohConnection};

    #[test]
    fn oversized_payload_is_truncated_before_it_enters_the_queue() {
        let bytes = zenoh::bytes::ZBytes::from(vec![b'x'; 1024 * 1024]);
        assert_eq!(
            limited_payload(&bytes, 8).unwrap(),
            Payload::String("xxxxxxxx".into())
        );
        // A valid JSON prefix of an oversized message must stay a truncated string.
        let json = zenoh::bytes::ZBytes::from(b"1234 ".to_vec());
        assert_eq!(
            limited_payload(&json, 4).unwrap(),
            Payload::String("1234".into())
        );
        assert!(matches!(
            limited_payload(&json, 5).unwrap(),
            Payload::Json(_)
        ));
    }

    #[test]
    #[ignore = "requires ZENOHUI_TEST_ROUTER and network access"]
    fn remote_router_is_listed_without_multicast() {
        let endpoint = std::env::var("ZENOHUI_TEST_ROUTER").expect("set ZENOHUI_TEST_ROUTER");
        check_router_connection(endpoint);
    }

    #[test]
    fn local_router_reconnect_preserves_session_on_failure_and_closes_old_workers() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let endpoint = format!("tcp/127.0.0.1:{port}");
        let mut config = Config::default();
        config.insert_json5("mode", "\"router\"").unwrap();
        config
            .insert_json5("scouting/multicast/enabled", "false")
            .unwrap();
        config
            .insert_json5("scouting/gossip/enabled", "false")
            .unwrap();
        config
            .insert_json5(
                "listen/endpoints",
                &serde_json::to_string(&[&endpoint]).unwrap(),
            )
            .unwrap();
        let router = zenoh::open(config).wait().unwrap();
        check_router_connection(endpoint);
        router.close().wait().unwrap();
    }

    fn check_router_connection(endpoint: String) {
        let (info, session) = crate::zenoh_client::connect(ZenohConnection {
            mode: SessionMode::Client,
            peer: vec![endpoint],
            listen: vec![],
            no_multicast: true,
        })
        .unwrap();
        let old_session = Arc::new(session);
        let thread = ZenohThread::new(
            Arc::clone(&old_session),
            &["zenohui/test/**".into()],
            8000,
            1000,
            info.config.clone(),
        )
        .unwrap();
        let cache = thread.get_scouting();
        assert!(cache.routers_len() > 0);
        assert!(cache.routers_list().iter().all(|router| router.connected));
        drop(cache);
        let mut app = crate::interactive::App::new(&info, thread);
        let key = |code| KeyEvent::new(code, KeyModifiers::NONE);
        app.on_key(key(KeyCode::Char('4'))).unwrap();
        app.settings.peers = "invalid/endpoint".into();
        app.on_key(key(KeyCode::F(5))).unwrap();
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 35)).unwrap();
        let wait =
            |app: &mut crate::interactive::App,
             terminal: &mut ratatui::Terminal<ratatui::backend::TestBackend>| {
                let deadline = std::time::Instant::now() + Duration::from_secs(15);
                while app.reconnect.is_some() {
                    terminal.draw(|frame| app.draw(frame)).unwrap();
                    assert!(std::time::Instant::now() < deadline, "reconnect timed out");
                    thread::sleep(Duration::from_millis(20));
                }
            };
        wait(&mut app, &mut terminal);
        assert!(app.settings.message.contains("Connection failed"));
        assert!(!old_session.is_closed());
        assert!(app.zenoh_thread.get_scouting().routers_len() > 0);
        app.settings.peers = info.connection.peer.join(", ");
        app.on_key(key(KeyCode::F(5))).unwrap();
        wait(&mut app, &mut terminal);
        assert!(app.settings.message.starts_with("Connected"));
        assert!(old_session.is_closed());
        assert!(app.zenoh_thread.get_scouting().routers_len() > 0);
        app.on_key(key(KeyCode::Char('2'))).unwrap();
        terminal.draw(|frame| app.draw(frame)).unwrap();
        let rendered: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(ratatui::buffer::Cell::symbol)
            .collect();
        assert!(rendered.contains("Connected"));
        drop(app);
    }
}
