pub struct HttpAcceptHeader {
    r: regex::Regex,
}

impl HttpAcceptHeader {
    pub fn new() -> Self {
        Self {
            r: regex::Regex::new(
                r"^\s*(?:\*|([^\s/;]+))\s*/\s*(?:\*|([^\s/;]+))\s*(?:;.*)?$",
            )
            .expect("regex"),
        }
    }

    pub fn parse<'s, 'a>(
        &'s self,
        accept_header: &'a str,
    ) -> Vec<(Option<&'a str>, Option<&'a str>)> {
        let mut res: Vec<(Option<&'a str>, Option<&'a str>)> = Vec::new();
        for s in accept_header.split(',') {
            let Some(m) = self.r.captures(s) else {
                continue;
            };
            res.push((
                m.get(1).as_ref().map(regex::Match::as_str),
                m.get(2).as_ref().map(regex::Match::as_str),
            ));
        }
        res
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn example_from_mozilla_docs() {
        // https://developer.mozilla.org/en-US/docs/Web/HTTP/Reference/Headers/Accept
        let s =
            "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8";
        let parsed = HttpAcceptHeader::new().parse(s);
        assert_eq!(
            parsed,
            vec![
                (Some("text"), Some("html")),
                (Some("application"), Some("xhtml+xml")),
                (Some("application"), Some("xml")),
                (None, None)
            ]
        );
    }

    #[test]
    fn first_component_wildcard() {
        let s = "*/html;q=0.9;q=0.8";
        let parsed = HttpAcceptHeader::new().parse(s);
        assert_eq!(parsed, vec![(None, Some("html")),]);
    }

    #[test]
    fn second_component_wildcard() {
        let s = "text/*;q=0.9;q=0.8";
        let parsed = HttpAcceptHeader::new().parse(s);
        assert_eq!(parsed, vec![(Some("text"), None),]);
    }
}
