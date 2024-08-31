use clap::Parser;

use std::io::Write;

use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(author, version)]
struct Args {
    /// Whether to allow running code from the package
    #[clap(long)]
    trust: bool,

    /// Whether to enable debug logging
    #[clap(long)]
    debug: bool,

    /// Whether to enable trace logging
    #[clap(long)]
    trace: bool,

    /// Do not probe external services
    #[clap(long)]
    disable_net_access: bool,

    /// Check guesssed metadata against external sources
    #[clap(long)]
    check: bool,

    /// Path to sources
    #[clap(default_value = ".")]
    path: PathBuf,

    /// Scan for metadata rather than printing results
    #[clap(long)]
    scan: bool,

    /// Scan specified homepage rather than current directory
    #[clap(long)]
    from_homepage: Option<url::Url>,

    /// Find data based on specified repology id
    #[clap(long)]
    from_repology: Option<String>,

    /// Pull in external (not maintained by upstream) directory data
    #[clap(long)]
    consult_external_directory: bool,
}

fn main() {
    let args = Args::parse();

    env_logger::builder()
        .format(|buf, record| writeln!(buf, "{}", record.args()))
        .filter(
            None,
            if args.trace {
                log::LevelFilter::Trace
            } else if args.debug {
                log::LevelFilter::Debug
            } else {
                log::LevelFilter::Info
            },
        )
        .init();

    if let Some(from_homepage) = args.from_homepage {
        for d in upstream_ontologist::homepage::guess_from_homepage(&from_homepage).unwrap() {
            println!(
                "{}: {:?} - certainty {} (from {:?})",
                d.datum.field(),
                d.datum,
                d.certainty
                    .map_or_else(|| "unknown".to_string(), |d| d.to_string()),
                d.origin
            );
        }
    } else if let Some(id) = args.from_repology {
        for d in upstream_ontologist::repology::find_upstream_from_repology(&id).unwrap() {
            println!(
                "{}: {:?} - certainty {} (from {:?})",
                d.datum.field(),
                d.datum,
                d.certainty
                    .map_or_else(|| "unknown".to_string(), |d| d.to_string()),
                d.origin
            );
        }
    } else if args.scan {
        for entry in upstream_ontologist::guess_upstream_info(
            &args.path.canonicalize().unwrap(),
            Some(args.trust),
        ) {
            let entry = entry.unwrap();
            println!(
                "{}: {:?} - certainty {}{}",
                entry.datum.field(),
                entry.datum,
                entry
                    .certainty
                    .map_or("unknown".to_string(), |c| c.to_string()),
                entry
                    .origin
                    .map_or_else(|| "".to_string(), |o| format!(" (from {:?})", o))
            );
        }
    } else {
        let metadata = match upstream_ontologist::guess_upstream_metadata(
            &args.path.canonicalize().unwrap(),
            Some(args.trust),
            Some(!args.disable_net_access),
            Some(args.consult_external_directory),
            Some(args.check),
        ) {
            Ok(m) => m,
            Err(upstream_ontologist::ProviderError::ParseError(e)) => {
                eprintln!("Error parsing metadata: {}", e);
                std::process::exit(1);
            }
            Err(upstream_ontologist::ProviderError::IoError(e)) => {
                eprintln!("I/O Error: {}", e);
                std::process::exit(1);
            }
            Err(upstream_ontologist::ProviderError::Other(e)) => {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
            Err(upstream_ontologist::ProviderError::HttpJsonError(e)) => {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
            Err(upstream_ontologist::ProviderError::ExtrapolationLimitExceeded(l)) => {
                eprintln!("Extraoplation limit exceeded: {}", l);
                std::process::exit(1);
            }
        };

        let out = serde_yaml::to_value(&metadata).unwrap();

        std::io::stdout()
            .write_all(serde_yaml::to_string(&out).unwrap().as_bytes())
            .unwrap();
    }
}
