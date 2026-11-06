//! The HTTP transport for remotes: GroveHub, or any server speaking Grove's
//! remote protocol (see `grove_core::remote`).
//!
//! A remote URL is the repository's page, `http://host/owner/name`; the API
//! lives at `http://host/api/repos/owner/name`. Anything that isn't an
//! `http(s)://` URL is a folder holding another Grove repository.

use anyhow::{anyhow, bail, Context, Result};
use grove_core::*;
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use std::path::Path;

pub struct Http {
    api: String,
}

impl Http {
    pub fn new(url: &str) -> Result<Http> {
        let url = url.trim_end_matches('/').trim_end_matches(".git");
        let (scheme, rest) = url
            .split_once("://")
            .ok_or_else(|| anyhow!("{url} is not a URL"))?;
        let (host, path) = rest.split_once('/').ok_or_else(|| {
            anyhow!("{url} names a server, not a repository (want http://host/owner/name)")
        })?;
        let path = path.trim_start_matches("api/repos/");
        if path.split('/').count() != 2 {
            bail!("{url} should look like {scheme}://{host}/owner/name");
        }
        Ok(Http {
            api: format!("{scheme}://{host}/api/repos/{path}"),
        })
    }

    fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        answer(ureq::get(&format!("{}{path}", self.api)).call())
    }

    fn post<T: DeserializeOwned>(&self, path: &str, body: Value) -> Result<T> {
        answer(ureq::post(&format!("{}{path}", self.api)).send_json(body))
    }
}

fn answer<T: DeserializeOwned>(r: std::result::Result<ureq::Response, ureq::Error>) -> Result<T> {
    match r {
        Ok(resp) => resp
            .into_json()
            .context("the server sent something that isn't JSON"),
        Err(ureq::Error::Status(code, resp)) => {
            let text = resp.into_string().unwrap_or_default();
            let msg = serde_json::from_str::<Value>(&text)
                .ok()
                .and_then(|v| v["error"].as_str().map(str::to_string))
                .unwrap_or(text);
            bail!("the server said {code}: {msg}")
        }
        Err(e) => Err(anyhow!("could not reach the server: {e}")),
    }
}

impl Remote for Http {
    fn refs(&self) -> Result<RemoteRefs> {
        self.get("/refs")
    }
    fn fetch(&self, have: Option<&Hash>) -> Result<Fetched> {
        self.post("/fetch", json!({ "have": have }))
    }
    fn push(&self, main: &Hash, objects: Vec<Object>, actor: &Actor) -> Result<usize> {
        let v: Value = self.post(
            "/push",
            json!({ "main": main, "objects": objects, "actor": actor }),
        )?;
        Ok(v["landings"].as_u64().unwrap_or(0) as usize)
    }
    fn propose(&self, change: &Change, land: bool, actor: &Actor) -> Result<Proposal> {
        self.post(
            "/changes",
            json!({ "change": change, "land": land, "actor": actor }),
        )
    }
}

/// A remote by URL or folder.
pub fn open(url: &str) -> Result<Box<dyn Remote>> {
    if url.starts_with("http://") || url.starts_with("https://") {
        Ok(Box::new(Http::new(url)?))
    } else {
        Ok(Box::new(Folder::open(Path::new(url))?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repository_urls_map_to_the_api() {
        assert_eq!(
            Http::new("http://localhost:4600/harshit/grovehub")
                .unwrap()
                .api,
            "http://localhost:4600/api/repos/harshit/grovehub"
        );
        assert_eq!(
            Http::new("https://hub.example/acme/api.git/").unwrap().api,
            "https://hub.example/api/repos/acme/api"
        );
        assert!(Http::new("http://localhost:4600").is_err());
    }
}
