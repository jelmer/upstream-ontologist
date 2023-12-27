use clap::Parser;
use maplit::hashmap;
use std::io::Write;

use std::path::PathBuf;
use upstream_ontologist::UpstreamDatum;
use xmltree::{Element, Namespace, XMLNode};

const DOAP_NS: &str = "http://usefulinc.com/ns/doap";
const RDF_NS: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns";
const FOAF_NS: &str = "http://xmlns.com/foaf/0.1/";

fn rdf_resource(namespace: &Namespace, url: String) -> XMLNode {
    XMLNode::Element(Element {
        prefix: Some("rdf".to_string()),
        namespaces: Some(namespace.clone()),
        namespace: Some(RDF_NS.to_string()),
        name: "resource".to_string(),
        attributes: hashmap! {"rdf:resource".to_string() => url},
        children: vec![],
    })
}

fn doap_file_from_upstream_info(data: Vec<UpstreamDatum>) -> Element {
    let mut namespace = Namespace::empty();
    namespace.put("doap", DOAP_NS);
    namespace.put("rdf", RDF_NS);
    namespace.put("foaf", FOAF_NS);

    let mut repository = None;
    let mut repository_browse = None;

    let mut children = vec![];

    for upstream_datum in data {
        match upstream_datum {
            UpstreamDatum::Name(n) => {
                children.push(XMLNode::Element(Element {
                    prefix: Some("doap".to_string()),
                    namespaces: Some(namespace.clone()),
                    namespace: Some(DOAP_NS.to_string()),
                    name: "name".to_string(),
                    attributes: hashmap! {},
                    children: vec![XMLNode::Text(n)],
                }));
            }
            UpstreamDatum::Homepage(h) => {
                children.push(XMLNode::Element(Element {
                    prefix: Some("doap".to_string()),
                    namespaces: Some(namespace.clone()),
                    namespace: Some(DOAP_NS.to_string()),
                    name: "homepage".to_string(),
                    attributes: hashmap! {},
                    children: vec![rdf_resource(&namespace, h)],
                }));
            }
            UpstreamDatum::Summary(s) => {
                children.push(XMLNode::Element(Element {
                    prefix: Some("doap".to_string()),
                    namespaces: Some(namespace.clone()),
                    namespace: Some(DOAP_NS.to_string()),
                    name: "shortdesc".to_string(),
                    attributes: hashmap! {},
                    children: vec![XMLNode::Text(s)],
                }));
            }
            UpstreamDatum::Description(d) => {
                children.push(XMLNode::Element(Element {
                    prefix: Some("doap".to_string()),
                    namespaces: Some(namespace.clone()),
                    namespace: Some(DOAP_NS.to_string()),
                    name: "description".to_string(),
                    attributes: hashmap! {},
                    children: vec![XMLNode::Text(d)],
                }));
            }
            UpstreamDatum::Download(d) => {
                children.push(XMLNode::Element(Element {
                    prefix: Some("doap".to_string()),
                    namespaces: Some(namespace.clone()),
                    namespace: Some(DOAP_NS.to_string()),
                    name: "download-page".to_string(),
                    attributes: hashmap! {},
                    children: vec![rdf_resource(&namespace, d)],
                }));
            }
            UpstreamDatum::MailingList(ml) => {
                children.push(XMLNode::Element(Element {
                    prefix: Some("doap".to_string()),
                    namespaces: Some(namespace.clone()),
                    namespace: Some(DOAP_NS.to_string()),
                    name: "mailing-list".to_string(),
                    attributes: hashmap! {},
                    children: vec![rdf_resource(&namespace, ml)],
                }));
            }
            UpstreamDatum::BugDatabase(bd) => {
                children.push(XMLNode::Element(Element {
                    prefix: Some("doap".to_string()),
                    namespaces: Some(namespace.clone()),
                    namespace: Some(DOAP_NS.to_string()),
                    name: "bug-database".to_string(),
                    attributes: hashmap! {},
                    children: vec![rdf_resource(&namespace, bd)],
                }));
            }
            UpstreamDatum::Screenshots(us) => {
                for u in us {
                    children.push(XMLNode::Element(Element {
                        prefix: Some("doap".to_string()),
                        namespaces: Some(namespace.clone()),
                        namespace: Some(DOAP_NS.to_string()),
                        name: "screenshots".to_string(),
                        attributes: hashmap! {},
                        children: vec![rdf_resource(&namespace, u)],
                    }));
                }
            }
            UpstreamDatum::SecurityContact(sc) => {
                children.push(XMLNode::Element(Element {
                    prefix: Some("doap".to_string()),
                    namespaces: Some(namespace.clone()),
                    namespace: Some(DOAP_NS.to_string()),
                    name: "security-contact".to_string(),
                    attributes: hashmap! {},
                    children: vec![rdf_resource(&namespace, sc)],
                }));
            }
            UpstreamDatum::Wiki(r) => {
                children.push(XMLNode::Element(Element {
                    prefix: Some("doap".to_string()),
                    namespaces: Some(namespace.clone()),
                    namespace: Some(DOAP_NS.to_string()),
                    name: "wiki".to_string(),
                    attributes: hashmap! {},
                    children: vec![rdf_resource(&namespace, r)],
                }));
            }
            UpstreamDatum::Repository(r) => {
                repository = Some(r);
            }
            UpstreamDatum::RepositoryBrowse(r) => {
                repository_browse = Some(r);
            }
            _ => {}
        }
    }

    if repository.is_some() || repository_browse.is_some() {
        let mut git_repo_el = Element {
            prefix: Some("doap".to_string()),
            namespaces: Some(namespace.clone()),
            namespace: Some(DOAP_NS.to_string()),
            name: "GitRepository".to_string(),
            attributes: hashmap! {},
            children: vec![],
        };

        if let Some(r) = repository {
            git_repo_el.children.push(XMLNode::Element(Element {
                prefix: Some("doap".to_string()),
                namespaces: Some(namespace.clone()),
                namespace: Some(DOAP_NS.to_string()),
                name: "location".to_string(),
                attributes: hashmap! {},
                children: vec![rdf_resource(&namespace, r)],
            }));
        }

        if let Some(b) = repository_browse {
            git_repo_el.children.push(XMLNode::Element(Element {
                prefix: Some("doap".to_string()),
                namespaces: Some(namespace.clone()),
                namespace: Some(DOAP_NS.to_string()),
                name: "browse".to_string(),
                attributes: hashmap! {},
                children: vec![rdf_resource(&namespace, b)],
            }));
        }

        children.push(XMLNode::Element(Element {
            prefix: Some("doap".to_string()),
            namespaces: Some(namespace.clone()),
            namespace: Some(DOAP_NS.to_string()),
            name: "repository".to_string(),
            attributes: hashmap! {},
            children: vec![XMLNode::Element(git_repo_el)],
        }));
    }

    Element {
        prefix: Some("doap".to_string()),
        namespaces: Some(namespace),
        namespace: Some(DOAP_NS.to_string()),
        name: "Project".to_string(),
        attributes: hashmap! {},
        children,
    }
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

    /// Consult external directory
    #[clap(long)]
    consult_external_directory: bool,
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

    let upstream_info = upstream_ontologist::get_upstream_info(
        path.as_path(),
        Some(args.trust),
        Some(!args.disable_net_access),
        Some(args.consult_external_directory),
        Some(args.check),
    )
    .unwrap();

    let el = doap_file_from_upstream_info(upstream_info.into());

    use xmltree::EmitterConfig;

    let config = EmitterConfig::new()
        .perform_indent(true)
        .normalize_empty_elements(true);

    el.write_with_config(&mut std::io::stdout(), config)
        .unwrap();
}
