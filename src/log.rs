use std::sync::{mpsc, Arc};
use std::thread;

use zenoh::handlers::fifo::FifoChannel;
use zenoh::sample::SampleKind;
use zenoh::{Session, Wait};

use serde::Serialize;

use crate::format;
use crate::payload::Payload;
use crate::zenoh_client::Time;

#[derive(Serialize)]
struct JsonLog {
    time: Time,
    kind: &'static str,
    keyexpr: String,
    size: usize,
    payload: Payload,
}

pub fn show(session: Arc<Session>, keyexprs: Vec<String>, json: bool) -> anyhow::Result<()> {
    let (tx, rx) = mpsc::channel();

    for keyexpr in keyexprs {
        let session = Arc::clone(&session);
        let tx = tx.clone();
        thread::Builder::new()
            .name(format!("zenoh log {keyexpr}"))
            .spawn(move || {
                let subscriber = match session
                    .declare_subscriber(&keyexpr)
                    .with(FifoChannel::default())
                    .wait()
                {
                    Ok(subscriber) => subscriber,
                    Err(err) => {
                        eprintln!("Failed to subscribe to {keyexpr}: {err}");
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
                            eprintln!("Subscriber error: {err}");
                            break;
                        }
                    }
                }
            })
            .expect("should be able to spawn subscriber thread");
    }

    drop(tx);

    for sample in rx {
        let kind = format::kind(sample.kind());
        let time = if sample.kind() == SampleKind::Delete {
            Time::Unknown
        } else {
            Time::new_now()
        };
        let keyexpr = sample.key_expr().as_str().to_owned();
        let payload = sample.payload().to_bytes().to_vec();
        let size = payload.len();
        let payload = Payload::unlimited(payload);

        if json {
            let json = serde_json::to_string(&JsonLog {
                time,
                kind,
                keyexpr,
                size,
                payload,
            })
            .expect("Should be able to format log line as JSON");
            println!("{json}");
        } else {
            println!("{time:12} Kind:{kind:6} {keyexpr:50} Payload({size:>3}): {payload}");
        }
    }

    Ok(())
}
