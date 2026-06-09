use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(name = "crab")]
#[command(version)]
#[command(
    about = "Crab, a Rust-native local agent runtime with goal-state loops and worker delegation"
)]
pub struct Cli {
    #[command(flatten)]
    pub global: GlobalOptions,
    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Debug, Clone, Args, Default)]
pub struct GlobalOptions {
    #[arg(long, global = true)]
    pub provider: Option<String>,
    #[arg(long, global = true)]
    pub model: Option<String>,
    #[arg(long, global = true)]
    pub base_url: Option<String>,
    #[arg(long, global = true)]
    pub api_key: Option<String>,
    #[arg(long, global = true)]
    pub workspace: Option<PathBuf>,
    #[arg(long, global = true)]
    pub data_dir: Option<PathBuf>,
    #[arg(long, global = true)]
    pub session: Option<String>,
    #[arg(long, global = true)]
    pub max_iterations: Option<usize>,
    #[arg(long, global = true)]
    pub enable_shell: bool,
}

#[derive(Debug, Clone, Subcommand)]
pub enum Commands {
    Chat(ChatArgs),
    DebugContext(DebugContextArgs),
    Doctor(DoctorArgs),
    MemoryCompress(MemoryCompressArgs),
    Profile,
    RuntimeStatus,
    RuntimeStart,
    RuntimeRepair,
    RuntimeReset,
    DesktopBridge,
    #[command(hide = true)]
    Office2PdfRender(Office2PdfRenderArgs),
}

#[derive(Debug, Clone, Args)]
pub struct ChatArgs {
    #[arg(long)]
    pub prompt: Option<String>,
}

#[derive(Debug, Clone, Args)]
pub struct DebugContextArgs {
    #[arg(long)]
    pub prompt: String,
    #[arg(long, default_value_t = false)]
    pub execute: bool,
}

#[derive(Debug, Clone, Args)]
pub struct DoctorArgs {
    #[arg(long, default_value_t = false)]
    pub json: bool,
}

#[derive(Debug, Clone, Args)]
pub struct MemoryCompressArgs {
    #[arg(long)]
    pub session_id: Option<String>,
    #[arg(long, default_value = "")]
    pub query: String,
    #[arg(long, default_value = "markdown")]
    pub format: String,
}

#[derive(Debug, Clone, Args)]
pub struct Office2PdfRenderArgs {
    pub input: PathBuf,
    pub output: PathBuf,
}
