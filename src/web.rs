// mirra (c) Nikolas Wipper 2022

/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

use std::convert::Infallible;
use std::env;
use std::io::Result;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use tokio_util::codec::{BytesCodec, FramedRead};
use hyper::{Body, Method, Request, Response, Server, StatusCode};
use hyper::service::{make_service_fn, service_fn};
use log::warn;
use tokio::fs::File;

use crate::config::Config;
use crate::LocalKeys;

const STYLE: &str = include_str!("web/style.css");
const LAYOUT: &str = include_str!("web/index.html");

fn make_description(name: &String, module: &Option<String>) -> String {
    if let Some(module) = module {
        format!("Share {}'s {} module via <a href=\"https://github.com/Not-Nik/mirra\">mirra</a>.", name, module)
    } else {
        format!("Share any of {}'s modules via <a href=\"https://github.com/Not-Nik/mirra\">mirra</a>.", name)
    }
}

fn make_list_page(entries: Vec<(String, String, bool)>, module: Option<String>, host: Option<String>, config: Arc<Config>) -> Result<String> {
    let repeat_begin = LAYOUT.find("$(");
    let repeat_end = LAYOUT.find(")*");

    if repeat_begin.is_none() || repeat_end.is_none() || repeat_end.unwrap() < repeat_begin.unwrap() {
        // todo: proper error page
        return Ok("error".to_string());
    }

    let rb = repeat_begin.unwrap();
    let re = repeat_end.unwrap();

    let mut insertion_index = rb;

    let mut stripped_layout = LAYOUT.to_string();
    stripped_layout.replace_range(rb..re + 2, "");

    let s;
    stripped_layout = stripped_layout.replace("$title", "mirra")
        .replace("$name", &config.name)
        .replace("$desc", &make_description(&config.name, &module))
        .replace("$setup", if host.is_some() && module.is_some() {
            s = format!("mirra sync {} {}", host.as_ref().unwrap(), module.as_ref().unwrap());
            s.as_str()
        } else { "" });

    let repeat = LAYOUT.chars().skip(rb + 2).take(re - rb - 2).collect::<String>();

    for sync in entries {
        let str = repeat
            .replace("$path", &(sync.0 + ""))
            .replace("$info", &sync.1)
            .replace("$download", if sync.2 { "download" } else { "" });
        stripped_layout.insert_str(insertion_index, &str);
        insertion_index += str.len();
    }

    Ok(stripped_layout)
}

fn format_size(size: u64) -> String {
    if size < 1024 {
        size.to_string() + "B"
    } else if size < 1024 * 1024 {
        format!("{:.2}KiB", size as f64 / 1024.0)
    } else if size < 1024 * 1024 * 1024 {
        format!("{:.2}MiB", size as f64 / 1024.0 / 1024.0)
    } else if size < 1024 * 1024 * 1024 * 1024 {
        format!("{:.2}GiB", size as f64 / 1024.0 / 1024.0 / 1024.0)
    } else if size < 1024 * 1024 * 1024 * 1024 * 1024 {
        format!("{:.2}TiP", size as f64 / 1024.0 / 1024.0 / 1024.0 / 1024.0)
    } else {
        format!("{:.2}PiB", size as f64 / 1024.0 / 1024.0 / 1024.0 / 1024.0 / 1024.0)
    }
}

async fn list_directory(path: PathBuf, module: String, host: Option<String>, config: Arc<Config>) -> Result<String> {
    let mut list = tokio::fs::read_dir(path).await?;
    let mut entries = Vec::new();
    loop {
        // Get next directory entry
        let entry = list.next_entry().await?;
        if entry.is_none() { break; }
        if let Some(entry) = entry {
            let name = entry.file_name().into_string();
            if let Ok(mut name) = name {
                let is_dir = entry.path().is_dir();
                if is_dir {
                    name.push('/');
                }
                let metadata = entry.metadata().await;
                entries.push((name, if let Ok(metadata) = metadata {
                    format_size(metadata.len())
                } else {
                    "-".to_string()
                }, !is_dir));
            }
        }
    }
    make_list_page(entries, Some(module), host, config)
}

async fn handle(req: Request<Body>, config: Arc<Config>) -> Result<Response<Body>> {
    if req.method() != &Method::GET {
        return Ok(Response::builder().status(StatusCode::METHOD_NOT_ALLOWED).body(Body::empty()).unwrap());
    }

    let headers = req.headers();
    let host_header = headers.get("Host");
    let host = if let Some(host_header) = host_header {
        let x = host_header.to_str();
        if let Ok(x) = x {
            if x.contains(":") {
                Some(x.split(":").next().unwrap().to_string())
            } else {
                Some(x.to_string())
            }
        } else {
            None
        }
    } else {
        None
    };

    let uri = req.uri();
    let path = uri.path();

    if path == "/" {
        let mut modules = Vec::new();

        for share in &config.shares {
            modules.push((share.0.clone() + "/", "root is local".to_string(), false));
        }

        for sync in &config.syncs {
            modules.push((sync.0.clone() + "/", format!("root is <a href=\"//{}\">remote</a>", sync.1.address), false));
        }

        Ok(Response::new(Body::from(make_list_page(modules, None, host, config)?)))
    } else if path == "/style.css" {
        Ok(Response::new(STYLE.into()))
    } else {
        let mut s_path = path.chars().skip(1).collect::<String>();
        let mut dir: Option<PathBuf> = None;
        let mut init = false;
        let mut module: Option<String> = None;
        for share in &config.shares {
            if s_path.starts_with(share.0) {
                module = Some(share.0.to_string());
                s_path.replace_range(0..share.0.len(), &share.1.path);
                dir = Some(env::current_dir().unwrap().join(&s_path));
                init = true;
                break;
            }
        }

        if !init {
            for sync in &config.syncs {
                if s_path.starts_with(sync.0) {
                    module = Some(sync.0.to_string());
                    dir = Some(env::current_dir().unwrap().join(&s_path));
                    init = true;
                    break;
                }
            }
        }

        if !init || !dir.as_ref().unwrap().exists() {
            Ok(Response::new(Body::from("Empty")))
        } else {
            if dir.as_ref().unwrap().is_dir() {
                if !path.ends_with("/") {
                    Ok(Response::builder()
                        .status(StatusCode::PERMANENT_REDIRECT)
                        .header("Location", path.to_string() + "/")
                        .body(Body::empty()).unwrap())
                } else {
                    Ok(Response::new(Body::from(list_directory(dir.unwrap(), module.unwrap(), host, config).await?)))
                }
            } else {
                let file = File::open(dir.unwrap()).await.unwrap();
                let stream = FramedRead::new(file, BytesCodec::new());
                let body = Body::wrap_stream(stream);
                Ok(Response::new(body))
            }
        }
    }
}

pub async fn web(config: Arc<Config>, keys: Arc<LocalKeys>) -> Result<()> {

    // Construct our SocketAddr to listen on...
    let addr = SocketAddr::from(([0, 0, 0, 0], 80));

    // And a MakeService to handle each connection...
    let make_service = make_service_fn(move |_conn| {
        // yay moving a non-Copy object into two nested async closures
        let local_config = config.clone();
        //let local_keys = keys.clone();
        async move {
            Ok::<_, Infallible>(service_fn(move |req: Request<Body>| {
                let ll_config = local_config.clone();
                //let ll_keys = local_keys.clone();
                async move {
                    handle(req, ll_config.clone()).await
                }
            }))
        }
    });

    // Then bind and serve...
    let server = Server::bind(&addr).serve(make_service);

    // And run forever...
    if let Err(e) = server.await {
        warn!("{}", e);
    }

    Ok(())
}
