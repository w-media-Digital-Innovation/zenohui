use zenoh::config::WhatAmI;
use zenoh::{Config, Session, Wait};

use crate::cli::{SessionMode, ZenohConnection};

pub struct SessionInfo {
    pub connection: ZenohConnection,
    pub description: String,
    pub config: Config,
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
        let list = serde_json::to_string(&connection.peer)?;
        config
            .insert_json5("connect/endpoints", &list)
            .map_err(|err| anyhow::anyhow!("Invalid --peer endpoint list: {err}"))?;
    }

    if !connection.listen.is_empty() {
        let list = serde_json::to_string(&connection.listen)?;
        config
            .insert_json5("listen/endpoints", &list)
            .map_err(|err| anyhow::anyhow!("Invalid --listen endpoint list: {err}"))?;
    }

    if matches!(mode, WhatAmI::Client) && connection.peer.is_empty() && connection.listen.is_empty()
    {
        config
            .insert_json5("connect/endpoints", r#"["tcp/127.0.0.1:7447"]"#)
            .map_err(|err| anyhow::anyhow!("Invalid default peer endpoint: {err}"))?;
    }

    config
        .insert_json5(
            "scouting/multicast/enabled",
            if connection.no_multicast {
                "false"
            } else {
                "true"
            },
        )
        .map_err(|err| anyhow::anyhow!("Invalid multicast setting: {err}"))?;
    config
        .insert_json5("connect/timeout_ms", "5000")
        .map_err(|err| anyhow::anyhow!("Invalid connection timeout: {err}"))?;
    let session = zenoh::open(config.clone()).wait().map_err(|err| {
        anyhow::anyhow!("Failed to open Zenoh session. Check --peer/--listen/--mode options: {err}")
    })?;

    let info = SessionInfo {
        description: connection.describe(),
        connection,
        config: config.clone(),
    };

    Ok((info, session))
}
