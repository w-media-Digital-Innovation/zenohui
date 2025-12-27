use zenoh::{Session, Wait};

pub fn clean(session: &Session, keyexpr: &str, dry_run: bool) -> anyhow::Result<()> {
    let has_wildcards = keyexpr.contains('*');
    if dry_run {
        if has_wildcards {
            println!("Dry run: would delete key expression {keyexpr}");
        } else {
            println!("Dry run: would put empty payload and delete {keyexpr}");
        }
        return Ok(());
    }

    if has_wildcards {
        session
            .delete(keyexpr)
            .wait()
            .map_err(|err| anyhow::anyhow!(err))?;
        println!("Deleted key expression {keyexpr}");
        return Ok(());
    }

    session
        .put(keyexpr, Vec::<u8>::new())
        .wait()
        .map_err(|err| anyhow::anyhow!(err))?;
    session
        .delete(keyexpr)
        .wait()
        .map_err(|err| anyhow::anyhow!(err))?;
    println!("Cleaned {keyexpr}");
    Ok(())
}
