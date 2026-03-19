use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "ccline")]
#[command(version, about = "High-performance Claude Code StatusLine")]
pub struct Cli {
    /// Enter TUI configuration mode
    #[arg(short = 'c', long = "config")]
    pub config: bool,

    /// Set theme
    #[arg(short = 't', long = "theme")]
    pub theme: Option<String>,

    /// Patch Claude Code cli.js to disable context warnings
    #[arg(long = "patch")]
    pub patch: Option<String>,

    /// Log output path (file or directory)
    #[arg(long = "logto")]
    pub logto: Option<String>,

    /// Log level: debug | info | error (default: info)
    #[arg(long = "loglevel", default_value = "info")]
    pub loglevel: String,
}

impl Cli {
    pub fn parse_args() -> Self {
        Self::parse()
    }
}
