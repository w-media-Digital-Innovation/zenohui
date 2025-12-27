use zenoh::config::WhatAmI;
use zenoh::{Config, Session, Wait};

use crate::cli::{SessionMode, ZenohConnection};

pub struct SessionInfo {
    pub description: String,
}

pub fn connect(connection: ZenohConnection) -> anyhow::Result<(SessionInfo, Session)> {
    let mut config = Config::default();
    let mode = match connection.mode {
        SessionMode::Client => WhatAmI::Client,
        SessionMode::Peer => WhatAmI::Peer,
        SessionMode::Router => WhatAmI::Router,
    };
    let mode_value = format!(
        "\"{}\"",
        match mode {
            WhatAmI::Client => "client",
            WhatAmI::Peer => "peer",
            WhatAmI::Router => "router",
        }
    );
    config
        .insert_json5("mode", &mode_value)
        .map_err(|err| anyhow::anyhow!("Failed to set Zenoh mode: {err}"))?;

    if !connection.peer.is_empty() {
        let list = format!(
            "[{}]",
            connection
                .peer
                .iter()
                .map(|endpoint| format!("\"{endpoint}\""))
                .collect::<Vec<_>>()
                .join(", ")
        );
        config
            .insert_json5("connect/endpoints", &list)
            .map_err(|err| anyhow::anyhow!("Invalid --peer endpoint list: {err}"))?;
    }

    if !connection.listen.is_empty() {
        let list = format!(
            "[{}]",
            connection
                .listen
                .iter()
                .map(|endpoint| format!("\"{endpoint}\""))
                .collect::<Vec<_>>()
                .join(", ")
        );
        config
            .insert_json5("listen/endpoints", &list)
            .map_err(|err| anyhow::anyhow!("Invalid --listen endpoint list: {err}"))?;
    }

    if connection.peer.is_empty() && connection.listen.is_empty() {
        config
            .insert_json5("connect/endpoints", r#"["tcp/127.0.0.1:7447"]"#)
            .map_err(|err| anyhow::anyhow!("Invalid default peer endpoint: {err}"))?;
    }

    let session = zenoh::open(config)
        .wait()
        .map_err(|err| {
            anyhow::anyhow!(
                "Failed to open Zenoh session. Check --peer/--listen/--mode options: {err}"
            )
        })?;

    let info = SessionInfo {
        description: connection.describe(),
    };

    Ok((info, session))
}
