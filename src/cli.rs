use clap::{Args, Parser, Subcommand, ValueEnum, ValueHint};

#[allow(clippy::doc_markdown)]
#[derive(Debug, Subcommand)]
pub enum Subcommands {
    /// Delete values for a key expression.
    ///
    /// This sends an empty PUT followed by a DELETE for the key expression.
    #[command(visible_alias = "c", visible_alias = "clean")]
    Clean {
        /// Key expression which gets cleaned.
        ///
        /// Supports wildcards like 'foo/**'.
        #[arg(value_hint = ValueHint::Other)]
        keyexpr: String,

        /// Dont delete keys, only log what would be sent
        #[arg(long)]
        dry_run: bool,
    },

    /// Log values from subscribed key expressions to stdout
    #[command(visible_alias = "l")]
    Log {
        /// Key expressions to watch
        #[arg(
            env = "ZENOHUI_KEYEXPR",
            value_hint = ValueHint::Other,
            default_value = "**",
        )]
        keyexpr: Vec<String>,

        /// Output incoming samples as newline-delimited JSON
        #[arg(short, long)]
        json: bool,
    },

    /// Wait for the first sample on the given key expression(s) and return its payload to stdout.
    ///
    /// Returns exactly one payload of the first received sample on the given key expression(s).
    /// The key expression of the received sample is printed to stderr.
    /// This means that you can handle stdout and stderr separately.
    ///
    /// This can be helpful for scripting to get the current temperature reading and pipe it to somewhere else:
    ///
    /// `echo "The temperature is $(zenohui read-one room/temp)"`
    ///
    /// The output is the exact payload in its binary form.
    /// This might be valid ASCII / Unicode but could also be something not intended to be displayed on a terminal.
    /// For a human readable format use `--pretty` or `zenohui log`.
    #[command(visible_alias = "r", visible_alias = "read")]
    ReadOne {
        /// Key expressions to watch
        #[arg(
            env = "ZENOHUI_KEYEXPR",
            value_hint = ValueHint::Other,
            default_value = "**",
        )]
        keyexpr: Vec<String>,

        /// Parse the payload and print it in a human readable pretty form.
        ///
        /// This might not be useful for piping the data.
        #[arg(short, long)]
        pretty: bool,
    },

    /// Publish a value quickly
    #[command(visible_alias = "p", visible_alias = "pub")]
    Publish {
        /// Key expression to publish to
        #[arg(value_hint = ValueHint::Other)]
        keyexpr: String,

        /// Payload to be published.
        ///
        /// Reads from stdin when not specified.
        /// This allows file content to be sent via pipes like this (bash):
        ///
        /// `zenohui publish some/key </etc/hostname`
        ///
        /// `cowsay "I was here" | zenohui publish some/key`
        #[arg(value_hint = ValueHint::Unknown)]
        payload: Option<String>,
    },
}

#[allow(clippy::doc_markdown)]
#[derive(Debug, Parser)]
#[command(about, version)]
pub struct Cli {
    #[clap(subcommand)]
    pub subcommands: Option<Subcommands>,

    /// Key expressions to watch
    #[arg(
        env = "ZENOHUI_KEYEXPR",
        value_hint = ValueHint::Other,
        default_value = "**",
    )]
    pub keyexpr: Vec<String>,

    /// Truncate the payloads stored to the given size.
    ///
    /// Payloads bigger than that are truncated and not inspected for formats like JSON or MessagePack.
    /// Only their beginning up to the specified amount of bytes can be viewed.
    /// Increasing this value might result in higher memory consumption especially over time.
    #[arg(
        long,
        env = "ZENOHUI_PAYLOAD_SIZE_LIMIT",
        value_hint = ValueHint::Other,
        default_value_t = 8_000,
    )]
    pub payload_size_limit: usize,

    // Keep at the end to not mix the next_help_heading with other options
    #[command(flatten, next_help_heading = "Zenoh Connection")]
    pub zenoh_connection: ZenohConnection,
}

/// Arguments related to the Zenoh connection.
#[derive(Debug, Args)]
pub struct ZenohConnection {
    /// Explicit peers to connect to, e.g. tcp/127.0.0.1:7447
    #[arg(
        long,
        env = "ZENOHUI_PEER",
        value_hint = ValueHint::Other,
        value_name = "ENDPOINT",
        global = true,
    )]
    pub peer: Vec<String>,

    /// Explicit endpoints to listen on, e.g. tcp/0.0.0.0:7447
    #[arg(
        long,
        env = "ZENOHUI_LISTEN",
        value_hint = ValueHint::Other,
        value_name = "ENDPOINT",
        global = true,
    )]
    pub listen: Vec<String>,

    /// Zenoh session mode
    #[arg(
        long,
        env = "ZENOHUI_MODE",
        value_enum,
        default_value_t = SessionMode::Client,
        global = true,
    )]
    pub mode: SessionMode,
}

impl ZenohConnection {
    pub fn describe(&self) -> String {
        let mut parts = vec![format!("mode={}", self.mode.as_str())];
        if !self.peer.is_empty() {
            parts.push(format!("peer={}", self.peer.join(",")));
        }
        if !self.listen.is_empty() {
            parts.push(format!("listen={}", self.listen.join(",")));
        }
        parts.join(" ")
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum SessionMode {
    Client,
    Peer,
    Router,
}

impl SessionMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Client => "client",
            Self::Peer => "peer",
            Self::Router => "router",
        }
    }
}

#[test]
fn verify() {
    use clap::CommandFactory;
    Cli::command().debug_assert();
}
