// iOS has no command line to print usage on.
#[cfg(not(target_os = "ios"))]
pub const USAGE: &str = "usage: OFFICEXR_SESSION_COOKIE=<Cookie header> officexr-client \
--web-url <url> --realtime-url <ws url> --office-id <id>
(--cookie <Cookie header> also works, but shows the cookie in process listings)";

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct Config {
    /// Serves `/api/*` and `/models/*`.
    pub web_url: String,
    pub realtime_url: String,
    pub office_id: String,
    // ponytail: a session cookie pasted from a signed-in browser instead of a native login.
    pub session_cookie: String,
}

impl Config {
    pub fn from_env_and_args() -> Result<Self, String> {
        Self::parse(std::env::args().skip(1), |name| std::env::var(name).ok())
    }

    fn parse(
        args: impl IntoIterator<Item = String>,
        env: impl Fn(&str) -> Option<String>,
    ) -> Result<Self, String> {
        let mut flags = std::collections::HashMap::new();
        let mut args = args.into_iter();
        while let Some(flag) = args.next() {
            let value = args.next().ok_or(format!("{flag} needs a value"))?;
            flags.insert(flag, value);
        }
        let mut get = |flag: &str, var: &str| {
            flags
                .remove(flag)
                .or_else(|| env(var))
                .ok_or(format!("missing {flag} (or {var})"))
        };
        let config = Self {
            web_url: get("--web-url", "OFFICEXR_WEB_URL")?
                .trim_end_matches('/')
                .into(),
            realtime_url: get("--realtime-url", "OFFICEXR_REALTIME_URL")?,
            office_id: get("--office-id", "OFFICEXR_OFFICE_ID")?,
            session_cookie: get("--cookie", "OFFICEXR_SESSION_COOKIE")?,
        };
        match flags.keys().next() {
            Some(unknown) => Err(format!("unknown flag {unknown}")),
            None => Ok(config),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn flags_override_env() {
        let config = Config::parse(
            args(&["--web-url", "http://a/", "--office-id", "o"]),
            |var| Some(format!("env:{var}")),
        )
        .unwrap();
        assert_eq!(config.web_url, "http://a");
        assert_eq!(config.office_id, "o");
        assert_eq!(config.realtime_url, "env:OFFICEXR_REALTIME_URL");
    }

    #[test]
    fn rejects_missing_and_unknown_flags() {
        assert!(Config::parse(args(&[]), |_| None).is_err());
        assert!(Config::parse(args(&["--nope", "x"]), |_| Some("v".into())).is_err());
    }
}
