use crate::{ProviderError, UpstreamMetadata};

pub fn parse_repology_name(name: &str) -> Option<(&str, &str)> {
    let (family, name) = name.split_once(':')?;
    Some((family, name))
}

fn perl_name_to_module(name: &str) -> String {
    name.split('-')
        .map(|x| {
            let mut x = x.chars();
            x.next()
                .unwrap()
                .to_uppercase()
                .chain(x)
                .collect::<String>()
        })
        .collect::<Vec<String>>()
        .join("::")
}

pub async fn find_upstream_from_repology(name: &str) -> Result<UpstreamMetadata, ProviderError> {
    let (family, name) = parse_repology_name(name)
        .ok_or_else(|| ProviderError::Other("Invalid repology name".to_string()))?;

    match family {
        "python" => crate::providers::python::remote_pypi_metadata(name).await,
        "go" => crate::providers::go::remote_go_metadata(name),
        "ruby" => crate::providers::ruby::remote_rubygem_metadata(name).await,
        "node" => crate::providers::node::remote_npm_metadata(name).await,
        "perl" => crate::providers::perl::remote_cpan_data(&perl_name_to_module(name)).await,
        "rust" => crate::providers::rust::remote_crate_data(name).await,
        "haskell" => crate::providers::haskell::remote_hackage_data(name).await,
        "apmod" => Ok(UpstreamMetadata::new()),
        "coq" => Ok(UpstreamMetadata::new()),
        "cursors" => Ok(UpstreamMetadata::new()),
        "deadbeef" => Ok(UpstreamMetadata::new()),
        "emacs" => Ok(UpstreamMetadata::new()),
        "erlang" => Ok(UpstreamMetadata::new()),
        "fonts" => Ok(UpstreamMetadata::new()),
        "fortunes" => Ok(UpstreamMetadata::new()),
        "fusefs" => Ok(UpstreamMetadata::new()),
        "gimp" => Ok(UpstreamMetadata::new()),
        "gstreamer" => Ok(UpstreamMetadata::new()),
        "gtktheme" => Ok(UpstreamMetadata::new()),
        "raku" => Ok(UpstreamMetadata::new()),
        "ros" => Ok(UpstreamMetadata::new()),
        "haxe" => Ok(UpstreamMetadata::new()),
        "icons" => Ok(UpstreamMetadata::new()),
        "java" => Ok(UpstreamMetadata::new()),
        "js" => Ok(UpstreamMetadata::new()),
        "julia" => Ok(UpstreamMetadata::new()),
        "ladspa" => Ok(UpstreamMetadata::new()),
        "lisp" => Ok(UpstreamMetadata::new()),
        "lua" => Ok(UpstreamMetadata::new()),
        "lv2" => Ok(UpstreamMetadata::new()),
        "mingw" => Ok(UpstreamMetadata::new()),
        "nextcloud" => Ok(UpstreamMetadata::new()),
        "nginx" => Ok(UpstreamMetadata::new()),
        "nim" => Ok(UpstreamMetadata::new()),
        "ocaml" => Ok(UpstreamMetadata::new()),
        "opencpn" => Ok(UpstreamMetadata::new()),
        "rhythmbox" => Ok(UpstreamMetadata::new()),
        "texlive" => Ok(UpstreamMetadata::new()),
        "tryton" => Ok(UpstreamMetadata::new()),
        "vapoursynth" => Ok(UpstreamMetadata::new()),
        "vdr" => Ok(UpstreamMetadata::new()),
        "vim" => Ok(UpstreamMetadata::new()),
        "xdrv" => Ok(UpstreamMetadata::new()),
        "xemacs" => Ok(UpstreamMetadata::new()),
        name => {
            log::warn!("Unknown family: {}", name);
            Ok(UpstreamMetadata::new())
        }
    }
}
