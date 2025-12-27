use std::sync::Arc;

use clap::Parser;

mod clean;
mod cli;
mod format;
mod interactive;
mod log;
mod payload;
mod publish;
mod read_one;
mod zenoh_client;

fn main() -> anyhow::Result<()> {
    let matches = cli::Cli::parse();
    let (session_info, session) = zenoh_client::connect(matches.zenoh_connection)?;
    let session = Arc::new(session);

    match matches.subcommands {
        Some(cli::Subcommands::Clean { keyexpr, dry_run }) => {
            clean::clean(session.as_ref(), &keyexpr, dry_run)?;
        }
        Some(cli::Subcommands::Log { keyexpr, json }) => {
            log::show(Arc::clone(&session), keyexpr, json)?;
        }
        Some(cli::Subcommands::ReadOne { keyexpr, pretty }) => {
            read_one::show(Arc::clone(&session), keyexpr, pretty)?;
        }
        Some(cli::Subcommands::Publish { keyexpr, payload }) => {
            let payload = payload.map_or_else(
                || {
                    use std::io::Read;
                    let mut buffer = Vec::new();
                    std::io::stdin()
                        .read_to_end(&mut buffer)
                        .expect("Should be able to read the payload from stdin");
                    buffer
                },
                String::into_bytes,
            );
            publish::send(session.as_ref(), &keyexpr, payload)?;
        }
        None => {
            interactive::show(
                Arc::clone(&session),
                &session_info,
                matches.keyexpr,
                matches.payload_size_limit,
            )?;
        }
    }

    Ok(())
}
