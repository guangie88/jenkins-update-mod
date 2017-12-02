#[macro_use]
extern crate error_chain;
extern crate futures;
extern crate futures_cpupool;
extern crate hyper;

extern crate log4rs;
#[macro_use]
extern crate log;

#[macro_use]
extern crate serde_derive;
extern crate serde_json;
extern crate structopt;

#[macro_use]
extern crate structopt_derive;
extern crate toml;
extern crate url;
extern crate walkdir;

use futures::Future;
use futures_cpupool::CpuPool;
use hyper::client::Client;
use hyper::header::ContentLength;
use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process;
use structopt::StructOpt;
use url::Url;
use walkdir::WalkDir;

mod errors {
    error_chain! {
        errors {
        }
    }
}

use errors::*;

#[derive(Serialize, Deserialize, Debug)]
struct FileConfig {
    sync_root_dir_path: String,
    url_list_json_file_path: PathBuf,
    accepted_file_exts: Vec<String>,
    download_thread_count: u32,
}

#[derive(StructOpt, Debug)]
#[structopt(name = "Test", about = "Test program")]
struct ArgConfig {
    #[structopt(short = "c", long = "config", help = "File configuration path")]
    config_path: String,

    #[structopt(short = "l", long = "log-config", help = "Log configuration file path")]
    log_config_path: String,
}

fn remove_parents(dir_path: &Path) {
    let res = fs::remove_dir(dir_path);

    if res.is_ok() {
        remove_parents(dir_path.parent().unwrap());
    }
}

fn run() -> Result<()> {
    let arg_config = ArgConfig::from_args();

    log4rs::init_file(&arg_config.log_config_path, Default::default()).chain_err(|| {
        format!(
            "Unable to initialize log4rs logger with the given config file at '{}'",
            arg_config.log_config_path
        )
    })?;

    let read_from_file_into_string = |path: &Path| {
        let mut file =
            File::open(path).chain_err(|| format!("Unable to open file path at {:?}", path))?;

        let mut s = String::new();

        file.read_to_string(&mut s)
            .map(|_| s)
            .chain_err(|| "Unable to read file into string")
    };

    let config_str = read_from_file_into_string(Path::new(&arg_config.config_path))?;

    let config: FileConfig = toml::from_str(&config_str).chain_err(|| {
        format!(
            "Unable to parse config as required toml format: {}",
            config_str
        )
    })?;

    info!("Completed configuration initialization!");

    // write the body here
    let urls_str = read_from_file_into_string(&config.url_list_json_file_path)?;

    let urls: Vec<String> = serde_json::from_str(&urls_str).chain_err(|| {
        format!(
            "Error in parsing URL list from {:?}",
            config.url_list_json_file_path
        )
    })?;

    let url_download_path_pairs: Vec<_> = urls.into_iter()
        .map(|url| Url::parse(&url))
        .inspect(|url_res| {
            // log any erroneous URL and continue
            if let Err(e) = *url_res {
                error!("Unable to parse into URL: {}", e);
            }
        })
        .filter_map(|url_res| url_res.ok())
        .map(|url| {
            let download_path = format!("{}{}", config.sync_root_dir_path, url.path());
            (url, download_path)
        })
        .collect();

    // find all the existing paths for possible deletion for unused files
    let sync_root_dir = Path::new(&config.sync_root_dir_path);

    let filtered_paths: HashSet<_> = if sync_root_dir.is_dir() {
        WalkDir::new(&config.sync_root_dir_path)
            .into_iter()
            .filter_map(|entry| match entry {
                Ok(entry) => {
                    let accepted = match entry.path().extension() {
                        Some(ext) => config
                            .accepted_file_exts
                            .iter()
                            .any(|accepted_file_ext| ext == accepted_file_ext.as_str()),
                        None => false,
                    };

                    if accepted {
                        Some(entry)
                    } else {
                        None
                    }
                }

                Err(ref e) => {
                    error!("Error in walking entry: {}", e);
                    None
                }
            })
            .map(|entry| -> PathBuf { entry.path().to_owned() })
            .collect()
    } else {
        HashSet::new()
    };

    let to_download_paths: HashSet<_> = url_download_path_pairs
        .iter()
        .map(|&(_, ref download_path)| PathBuf::from(download_path))
        .collect();

    let unused_paths = filtered_paths.difference(&to_download_paths);

    for unused_path in unused_paths {
        let res = fs::remove_file(unused_path);

        match res {
            Ok(_) => {
                info!("Remove unused file at {:?}", unused_path);

                // continue to attempt to remove as much empty parent directories as possible
                if let Some(parent_dir_path) = unused_path.parent() {
                    remove_parents(parent_dir_path);
                }
            }
            Err(e) => error!("Unable to remove unused file: {}", e),
        }
    }

    // starts the download process
    let pool = CpuPool::new(config.download_thread_count as usize);

    let download_futs: Vec<_> = url_download_path_pairs
        .into_iter()
        .map(|(url, download_path)| {
            pool.spawn_fn(move || {
                let thread_run = move || -> Result<()> {
                    // performs HTTP request to get the file
                    let client = Client::new();
                    let url_str = format!("{}", url);

                    let mut resp = client
                        .get(url)
                        .send()
                        .chain_err(|| "Unable to perform HTTP request with URL")?;

                    let content_len = match resp.headers.get::<ContentLength>() {
                        Some(content_len) => content_len.0,
                        None => bail!("Unable to obtain HTTP response content length"),
                    };

                    let download_path_parent = match Path::new(&download_path).parent() {
                        Some(path) => path,
                        None => bail!(
                            "Unable to get parent path of download path '{}'",
                            download_path
                        ),
                    };

                    // check against existing file entry if present for similar content length
                    let found_file_len = {
                        let found_file_metadata = if Path::new(&download_path).exists() {
                            fs::metadata(&download_path).ok()
                        } else {
                            None
                        };

                        found_file_metadata.map(|meta| meta.len())
                    };

                    let same_content_opt = found_file_len.and_then(|file_len| {
                        if file_len == content_len {
                            Some(())
                        } else {
                            None
                        }
                    });

                    match same_content_opt {
                        Some(_) => info!(
                            "Content length {} of HTTP request '{}' \
                             same as file length of '{}', not downloading...",
                            content_len,
                            url_str,
                            download_path
                        ),
                        None => {
                            info!("Downloading '{}' -> '{}'", url_str, download_path);

                            fs::create_dir_all(download_path_parent).chain_err(|| {
                                format!(
                                    "Unable to create directory chain {:?}",
                                    download_path_parent
                                )
                            })?;

                            let mut download_file = File::create(&download_path).chain_err(|| {
                                format!(
                                    "Unable to create file at '{}' for saving URL response",
                                    download_path
                                )
                            })?;

                            // 512 KB
                            let mut resp_bytes = [0; 512 * 1024];

                            loop {
                                let read_res = resp.read(&mut resp_bytes);

                                let read_len = match read_res {
                                    Ok(0) => break,
                                    Err(e) => {
                                        error!("Unable to read some response content bytes: {}", e);
                                        break;
                                    }
                                    Ok(len) => len,
                                };

                                download_file
                                    .write_all(&resp_bytes[..read_len])
                                    .chain_err(|| {
                                        format!(
                                            "Unable to write bytes into download file path '{}'",
                                            download_path
                                        )
                                    })?;
                            }
                        }
                    }

                    Ok(())
                };

                let thread_res = thread_run();

                if let Err(ref e) = thread_res {
                    error!("Download error: {}", e);
                }

                thread_res
            })
        })
        .collect();

    for download_fut in download_futs {
        let _ = download_fut.wait();
    }

    Ok(())
}

fn main() {
    match run() {
        Ok(_) => {
            println!("Program completed!");
            process::exit(0)
        }

        Err(ref e) => {
            let stderr = &mut io::stderr();

            writeln!(stderr, "Error: {}", e).expect("Unable to write error into stderr!");

            for e in e.iter().skip(1) {
                writeln!(stderr, "- Caused by: {}", e)
                    .expect("Unable to write error causes into stderr!");
            }

            process::exit(1);
        }
    }
}
