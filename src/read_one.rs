use std::sync::{mpsc, Arc};
use std::thread;

use zenoh::handlers::fifo::FifoChannel;
use zenoh::sample::SampleKind;
use zenoh::{Session, Wait};

use crate::payload::Payload;

pub fn show(session: Arc<Session>, keyexprs: Vec<String>, pretty: bool) -> anyhow::Result<()> {
    let (tx, rx) = mpsc::channel();

    for keyexpr in keyexprs {
        let session = Arc::clone(&session);
        let tx = tx.clone();
        thread::Builder::new()
            .name(format!("zenoh read-one {keyexpr}"))
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
                        if sample.kind() == SampleKind::Delete {
                                continue;
                            }
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

    if let Ok(sample) = rx.recv() {
        eprintln!("{}", sample.key_expr().as_str());
        let payload = sample.payload().to_bytes().to_vec();
        if pretty {
            let payload = Payload::unlimited(payload);
            println!("{payload:#}");
        } else {
            use std::io::Write;
            std::io::stdout()
                .write_all(&payload)
                .expect("Should be able to write payload to stdout");
        }
    }

    Ok(())
}
