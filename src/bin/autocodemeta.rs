use clap::Parser;
use pyo3::types::PyDict;
use serde::Serialize;
use std::collections::HashSet;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use upstream_ontologist::UpstreamDatum;

fn get_upstream_info(
    path: &Path,
    trust_package: Option<bool>,
    net_access: Option<bool>,
    check: Option<bool>,
) -> Vec<UpstreamDatum> {
    pyo3::Python::with_gil(|py| {
        let m = py.import("upstream_ontologist.guess").unwrap();
        let get_upstream_info = m.getattr("get_upstream_info").unwrap();

        let kwargs = PyDict::new(py);

        if let Some(trust_package) = trust_package {
            kwargs.set_item("trust_package", trust_package).unwrap();
        }

        if let Some(check) = check {
            kwargs.set_item("check", check).unwrap();
        }

        if let Some(net_access) = net_access {
            kwargs.set_item("net_access", net_access).unwrap();
        }

        get_upstream_info
            .call((path,), Some(kwargs))
            .unwrap()
            .call_method0("items")
            .unwrap()
            .iter()
            .unwrap()
            .map(|x| x.unwrap().extract::<UpstreamDatum>().unwrap())
            .collect()
    })
}

#[derive(Serialize, Default)]
struct SoftwareSourceCode {
    name: Option<String>,
    version: Option<String>,
    #[serde(rename = "codeRepository")]
    code_repository: Option<String>,
    #[serde(rename = "issueTracker")]
    issue_tracker: Option<String>,
    license: Option<String>,
    description: Option<String>,
    // TODO(jelmer): Support setting contIntegration
    // TODO(jelmer): Support keywords
    // TODO(jelmer): Support funder
    // TODO(jelmer): Support funding
    // TODO(jelmer): Support creation date
    // TODO(jelmer): Support first release date
    // TODO(jelmer): Support unique identifier
    // TODO(jelmer): Support runtime platform
    // TODO(jelmer): Support other software requirements
    // TODO(jelmer): Support operating system
    // TODO(jelmer): Support development status
    // TODO(jelmer): Support reference publication
    // TODO(jelmer): Support part of
    // TODO(jelmer): Support Author
    #[serde(rename = "downloadUrl")]
    download_url: Option<String>,
    #[serde(rename = "relatedLink")]
    related_link: HashSet<String>,
}

fn codemeta_file_from_upstream_info(data: Vec<UpstreamDatum>) -> SoftwareSourceCode {
    let mut result = SoftwareSourceCode {
        ..Default::default()
    };
    for upstream_datum in data {
        match upstream_datum {
            UpstreamDatum::Name(n) => {
                result.name = Some(n);
            }
            UpstreamDatum::Homepage(h) => {
                result.related_link.insert(h);
            }
            UpstreamDatum::Description(d) => {
                result.description = Some(d);
            }
            UpstreamDatum::Download(d) => {
                result.download_url = Some(d);
            }
            UpstreamDatum::MailingList(ml) => {
                result.related_link.insert(ml);
            }
            UpstreamDatum::BugDatabase(bd) => {
                result.issue_tracker = Some(bd);
            }
            UpstreamDatum::Screenshots(us) => {
                for u in us {
                    result.related_link.insert(u);
                }
            }
            UpstreamDatum::Wiki(r) => {
                result.related_link.insert(r);
            }
            UpstreamDatum::Repository(r) => {
                result.code_repository = Some(r);
            }
            UpstreamDatum::RepositoryBrowse(r) => {
                result.related_link.insert(r);
            }
            UpstreamDatum::License(l) => {
                result.license = Some(format!("https://spdx.org/licenses/{}", l));
            }
            UpstreamDatum::Version(v) => {
                result.version = Some(v);
            }
            UpstreamDatum::Documentation(a) => {
                result.related_link.insert(a);
            }
            _ => {}
        }
    }
    result
}

#[derive(Parser, Debug)]
#[command(author, version)]
struct Args {
    /// Whether to allow running code from the package
    #[clap(long)]
    trust: bool,

    /// Whether to enable debug logging
    #[clap(long)]
    debug: bool,

    /// Do not probe external services
    #[clap(long)]
    disable_net_access: bool,

    /// Check guesssed metadata against external sources
    #[clap(long)]
    check: bool,

    /// Path to sources
    #[clap(default_value = ".")]
    path: PathBuf,
}

fn main() {
    let args = Args::parse();

    env_logger::builder()
        .format(|buf, record| writeln!(buf, "{}", record.args()))
        .filter(
            None,
            if args.debug {
                log::LevelFilter::Debug
            } else {
                log::LevelFilter::Info
            },
        )
        .init();

    pyo3::prepare_freethreaded_python();

    let path = args.path.canonicalize().unwrap();

    let upstream_info = get_upstream_info(
        path.as_path(),
        Some(args.trust),
        Some(!args.disable_net_access),
        Some(args.check),
    );

    let codemeta = codemeta_file_from_upstream_info(upstream_info);

    std::io::stdout()
        .write_all(serde_json::to_string_pretty(&codemeta).unwrap().as_bytes())
        .unwrap();
}