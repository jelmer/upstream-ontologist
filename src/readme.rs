use crate::{Certainty, UpstreamDatum, UpstreamDatumWithMetadata};
use regex::Regex;

pub fn skip_paragraph(para: &str) -> (bool, Vec<UpstreamDatumWithMetadata>) {
    let mut ret = Vec::<UpstreamDatumWithMetadata>::new();
    let re = Regex::new(r"(?ms)^See .* for more (details|information)\.").unwrap();
    if re.is_match(para) {
        return (true, ret);
    }

    let re = Regex::new(r"(?ms)^See .* for instructions").unwrap();
    if re.is_match(para) {
        return (true, ret);
    }

    let re = Regex::new(r"(?ms)^Please refer .*\.").unwrap();
    if re.is_match(para) {
        return (true, ret);
    }

    if let Some(m) = Regex::new(r"(?ms)^It is licensed under (.*)")
        .unwrap()
        .captures(para)
    {
        ret.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::License(m.get(1).unwrap().as_str().to_string()),
            certainty: Some(Certainty::Possible),
            origin: None,
        });
        return (true, ret);
    }

    if let Some(m) = Regex::new(r"(?ms)^License: (.*)").unwrap().captures(para) {
        ret.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::License(m.get(1).unwrap().as_str().to_string()),
            certainty: Some(Certainty::Likely),
            origin: None,
        });
        return (true, ret);
    }

    if let Some(m) =
        Regex::new(r"(?ms)^(Home page|homepage_url|Main website|Website|Homepage): (.*)")
            .unwrap()
            .captures(para)
    {
        let mut url = m.get(2).unwrap().as_str().to_string();
        if url.starts_with('<') && url.ends_with('>') {
            url = url[1..url.len() - 1].to_string();
        }
        ret.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Homepage(url),
            certainty: Some(Certainty::Likely),
            origin: None,
        });
        return (true, ret);
    }

    if Regex::new(r"(?ms)^More documentation .* at http.*")
        .unwrap()
        .is_match(para)
    {
        return (true, ret);
    }

    if let Some(m) =
        Regex::new(r"(?ms)^Documentation (can be found|is hosted|is available) (at|on) ([^ ]+)")
            .unwrap()
            .captures(para)
    {
        ret.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Documentation(m.get(3).unwrap().as_str().to_string()),
            certainty: Some(Certainty::Likely),
            origin: None,
        });
        return (true, ret);
    }

    if let Some(m) = Regex::new(
        r"(?ms)^Documentation for (.*)\s+(can\s+be\s+found|is\s+hosted)\s+(at|on)\s+([^ ]+)",
    )
    .unwrap()
    .captures(para)
    {
        ret.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Name(m.get(1).unwrap().as_str().to_string()),
            certainty: Some(Certainty::Possible),
            origin: None,
        });
        ret.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Documentation(m.get(4).unwrap().as_str().to_string()),
            certainty: Some(Certainty::Likely),
            origin: None,
        });
        return (true, ret);
    }

    if Regex::new(r"(?ms)^Documentation[, ].*found.*(at|on).*\.")
        .unwrap()
        .is_match(para)
    {
        return (true, ret);
    }

    if Regex::new(r"(?ms)^See (http.*|gopkg.in.*|github.com.*)")
        .unwrap()
        .is_match(para)
    {
        return (true, ret);
    }

    if Regex::new(r"(?ms)^Available on (.*)")
        .unwrap()
        .is_match(para)
    {
        return (true, ret);
    }

    if let Some(m) =
        Regex::new(r"(?ms)^This software is freely distributable under the (.*) license.*")
            .unwrap()
            .captures(para)
    {
        ret.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::License(m.get(1).unwrap().as_str().to_string()),
            certainty: Some(Certainty::Likely),
            origin: None,
        });
        return (true, ret);
    }

    if Regex::new(r"(?ms)^This .* is hosted at .*")
        .unwrap()
        .is_match(para)
    {
        return (true, ret);
    }

    if Regex::new(r"(?ms)^This code has been developed by .*")
        .unwrap()
        .is_match(para)
    {
        return (true, ret);
    }

    if para.starts_with("Download and install using:") {
        return (true, ret);
    }

    if Regex::new(r"(?ms)^Bugs should be reported by .*")
        .unwrap()
        .is_match(para)
    {
        return (true, ret);
    }

    if let Some(m) = Regex::new(r"(?ms)^The bug tracker can be found at (http[^ ]+[^.])")
        .unwrap()
        .captures(para)
    {
        ret.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::BugDatabase(m.get(1).unwrap().as_str().to_string()),
            certainty: Some(Certainty::Likely),
            origin: None,
        });
        return (true, ret);
    }

    if let Some(m) = Regex::new(r"(?ms)^Copyright (\(c\) |)(.*)")
        .unwrap()
        .captures(para)
    {
        ret.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Copyright(m.get(2).unwrap().as_str().to_string()),
            certainty: Some(Certainty::Possible),
            origin: None,
        });
        return (true, ret);
    }

    if Regex::new(r"(?ms)^You install .*").unwrap().is_match(para) {
        return (true, ret);
    }

    if Regex::new(r"(?ms)^This .* is free software; .*")
        .unwrap()
        .is_match(para)
    {
        return (true, ret);
    }

    if let Some(m) = Regex::new(r"(?ms)^Please report any bugs(.*) to <(.*)>")
        .unwrap()
        .captures(para)
    {
        ret.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::BugDatabase(m.get(2).unwrap().as_str().to_string()),
            certainty: Some(Certainty::Possible),
            origin: None,
        });
        return (true, ret);
    }

    if Regex::new(r"(?ms)^Share and Enjoy").unwrap().is_match(para) {
        return (true, ret);
    }

    let lines = para.lines().collect::<Vec<&str>>();
    if !lines.is_empty() && ["perl Makefile.PL", "make", "./configure"].contains(&lines[0].trim()) {
        return (true, ret);
    }

    if Regex::new(r"(?ms)^For further information, .*")
        .unwrap()
        .is_match(para)
    {
        return (true, ret);
    }

    if Regex::new(r"(?ms)^Further information .*")
        .unwrap()
        .is_match(para)
    {
        return (true, ret);
    }

    if let Some(m) = Regex::new(r"(?ms)^A detailed ChangeLog can be found.*:\s+(http.*)")
        .unwrap()
        .captures(para)
    {
        ret.push(UpstreamDatumWithMetadata {
            datum: UpstreamDatum::Changelog(m.get(1).unwrap().as_str().to_string()),
            certainty: Some(Certainty::Possible),
            origin: None,
        });
        return (true, ret);
    }

    (false, ret)
}
