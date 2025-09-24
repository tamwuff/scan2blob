pub struct HttpBasicAuth {
    r: regex::Regex,
}

impl HttpBasicAuth {
    pub fn new() -> Self {
        Self {
            r: regex::Regex::new(r"^\s*Basic\s+(.+?)\s*$").expect("regex"),
        }
    }

    pub fn parse(&self, auth_header: &str) -> Option<(String, String)> {
        let Some(m) = self.r.captures(auth_header) else {
            return None;
        };

        let Ok(userpass) = base64::Engine::decode(
            &base64::prelude::BASE64_STANDARD,
            m.get(1).unwrap().as_str(),
        ) else {
            return None;
        };

        let Ok(userpass) = String::from_utf8(userpass) else {
            return None;
        };

        let Some((user, pass)) = userpass.split_once(':') else {
            return None;
        };
        Some((user.into(), pass.into()))
    }
}
