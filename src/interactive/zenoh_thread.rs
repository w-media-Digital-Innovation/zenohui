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
        scout_config: Config,
    ) -> anyhow::Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let connection_err = Arc::new(RwLock::new(None));
        let history = Arc::new(RwLock::new(ZenohHistory::new()));
        let scouting = Arc::new(RwLock::new(ScoutCache::default()));
        let (tx, rx) = mpsc::channel();

        let mut workers = Vec::new();
        let subscribers = subscribe_keyexpr
            .iter()
            .map(|keyexpr| {
                let subscriber = session
                    .declare_subscriber(keyexpr.as_str())
                    .with(FifoChannel::default())
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
                                    if tx.send(sample).is_err() {
                                        break;
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
