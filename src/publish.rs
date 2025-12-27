use zenoh::{Session, Wait};

pub fn send(session: &Session, keyexpr: &str, payload: Vec<u8>) -> anyhow::Result<()> {
    session
        .put(keyexpr, payload)
        .wait()
        .map_err(|err| anyhow::anyhow!(err))?;
    Ok(())
}
