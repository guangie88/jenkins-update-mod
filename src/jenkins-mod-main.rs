#[macro_use]
extern crate error_chain;
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

use hyper::client::{Client, RedirectPolicy};
use serde_json::{Map, Value};
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process;
use structopt::StructOpt;

mod errors {
    error_chain! {
        errors {
        }
    }
}

use errors::*;

#[derive(Serialize, Deserialize, Debug)]
struct FileConfig {
    update_center_url: String,
    suppress_front: String,
    suppress_back: String,
    connection_check_url_change: String,
    url_replace_from: String,
    url_replace_into: String,
    auto_create_output_dir: bool,
    modified_json_file_path: PathBuf,
    url_list_json_file_path: PathBuf,
}

#[derive(StructOpt, Debug)]
#[structopt(name = "Test", about = "Test program")]
struct ArgConfig {
    #[structopt(short = "c", long = "config", help = "File configuration path")]
    config_path: String,

    #[structopt(short = "l", long = "log-config", help = "Log configuration file path")]
    log_config_path: String,
}

// const key names
const CONNECTION_CHECK_URL_KEY: &str = "connectionCheckUrl";
const CORE_KEY: &str = "core";
const PLUGINS_KEY: &str = "plugins";
const URL_KEY: &str = "url";

type MapStrVal = Map<String, Value>;

fn change_connection_check_url<S: Into<String>>(
    resp_outer_map: &mut MapStrVal,
    connection_check_url_change: S,
) -> Result<()> {
    let connection_check_url = match resp_outer_map.get_mut(CONNECTION_CHECK_URL_KEY) {
        Some(connection_check_url) => connection_check_url,
        None => bail!(format!(
            "Unable to find '{}' for changing connection URL",
            CONNECTION_CHECK_URL_KEY
        )),
    };

    let connection_check_url = match connection_check_url {
        &mut Value::String(ref mut connection_check_url) => connection_check_url,
        c => bail!(format!(
            "Expected '{}' to contain string value, but found content: {:?}",
            CONNECTION_CHECK_URL_KEY,
            c
        )),
    };

    *connection_check_url = connection_check_url_change.into();
    Ok(())
}

fn replace_url_impl(
    url_outer: &mut Value,
    outer_key: &str,
    url_replace_from: &str,
    url_replace_into: &str,
) -> Result<String> {
    let url_outer_map = match url_outer {
        &mut Value::Object(ref mut url_outer_map) => url_outer_map,
        c => bail!(format!(
            "Expected '{}' to be an object, but found content: {:?}",
            outer_key,
            c
        )),
    };

    let url = match url_outer_map.get_mut(URL_KEY) {
        Some(url) => url,
        None => bail!(format!(
            "Expected '{}' to be present for '{}'",
            URL_KEY,
            CORE_KEY
        )),
    };

    let url_str = match url {
        &mut Value::String(ref mut url_str) => url_str,
        c => bail!(format!(
            "Expected '{}' to contain string value, but found content: {:?}",
            URL_KEY,
            c
        )),
    };

    let orig_url = url_str.to_owned();
    *url_str = url_str.replace(url_replace_from, url_replace_into);

    Ok(orig_url)
}

fn replace_core_url(
    resp_outer_map: &mut MapStrVal,
    url_replace_from: &str,
    url_replace_into: &str,
) -> Result<String> {
    let mut core = match resp_outer_map.get_mut(CORE_KEY) {
        Some(core) => core,
        None => bail!(format!(
            "Unable to find '{}' for core URL replacement",
            CORE_KEY
        )),
    };

    replace_url_impl(&mut core, CORE_KEY, url_replace_from, url_replace_into)
}

fn replace_plugin_urls(
    resp_outer_map: &mut MapStrVal,
    url_replace_from: &str,
    url_replace_into: &str,
) -> Result<Vec<String>> {
    let plugins = match resp_outer_map.get_mut(PLUGINS_KEY) {
        Some(plugins) => plugins,
        None => bail!(format!(
            "Unable to find '{}' for core URL replacement",
            CORE_KEY
        )),
    };

    let plugins_obj = match plugins {
        &mut Value::Object(ref mut plugins_obj) => plugins_obj,
        c => bail!(format!(
            "Expected '{}' to be of object type, but found content: {:?}",
            PLUGINS_KEY,
            c
        )),
    };

    let mut orig_urls = Vec::new();

    for (key, mut plugin) in plugins_obj.iter_mut() {
        let orig_url = replace_url_impl(plugin, key, url_replace_from, url_replace_into)?;
        orig_urls.push(orig_url);
    }

    Ok(orig_urls)
}

fn run() -> Result<()> {
    let arg_config = ArgConfig::from_args();

    log4rs::init_file(&arg_config.log_config_path, Default::default()).chain_err(|| {
        format!(
            "Unable to initialize log4rs logger with the given config file at '{}'",
            arg_config.log_config_path
        )
    })?;

    let config_str = {
        let mut config_file = File::open(&arg_config.config_path).chain_err(|| {
            format!(
                "Unable to open config file path at {:?}",
                arg_config.config_path
            )
        })?;

        let mut s = String::new();

        config_file
            .read_to_string(&mut s)
            .map(|_| s)
            .chain_err(|| "Unable to read config file into string")?
    };

    let config: FileConfig = toml::from_str(&config_str).chain_err(|| {
        format!(
            "Unable to parse config as required toml format: {}",
            config_str
        )
    })?;

    info!("Completed configuration initialization!");

    // write the body here
    let mut client = Client::new();
    client.set_redirect_policy(RedirectPolicy::FollowAll);

    let mut resp = client.get(&config.update_center_url).send().chain_err(|| {
        format!(
            "Unable to perform HTTP request with URL string '{}'",
            config.update_center_url
        )
    })?;

    let mut resp_str = String::new();
    resp.read_to_string(&mut resp_str)
        .chain_err(|| "Unable to read HTTP response into string")?;

    let resp_str = resp_str;

    let trimmed_resp_str = resp_str
        .trim_left_matches(&config.suppress_front)
        .trim_right_matches(&config.suppress_back);

    // JSON parsing all the way
    let mut resp_json: Value = serde_json::from_str(trimmed_resp_str)
        .chain_err(|| "Unable to parse trimmed JSON string into JSON value.")?;

    // to stop borrowing early
    let (core_orig_url, mut plugin_urls) = {
        let mut resp_outer_map = match resp_json {
            Value::Object(ref mut resp_outer_map) => resp_outer_map,
            c => bail!(format!(
                "Expected outer most JSON to be of Object type, but found content: {:?}",
                c
            )),
        };

        change_connection_check_url(
            &mut resp_outer_map,
            config.connection_check_url_change.to_owned(),
        )?;
        let core_orig_url = replace_core_url(
            &mut resp_outer_map,
            &config.url_replace_from,
            &config.url_replace_into,
        )?;
        let plugin_urls = replace_plugin_urls(
            &mut resp_outer_map,
            &config.url_replace_from,
            &config.url_replace_into,
        )?;

        (core_orig_url, plugin_urls)
    };

    // combine both the core + plugin links
    let mut urls = vec![core_orig_url];
    urls.append(&mut plugin_urls);
    let urls = urls;

    // write the modified JSON file
    if config.auto_create_output_dir {
        let create_parent_dir_if_present = |dir_opt: Option<&Path>| {
            let dir_opt = dir_opt.and_then(|dir| {
                // ignore if the directory has already been created
                if Path::new(dir).is_dir() {
                    None
                } else {
                    Some(dir)
                }
            });

            match dir_opt {
                Some(dir) => {
                    info!("Creating directory chain: {:?}", dir);

                    fs::create_dir_all(dir)
                        .chain_err(|| format!("Unable to create directory chain: {:?}", dir))
                }

                None => Ok(()),
            }
        };

        create_parent_dir_if_present(config.modified_json_file_path.parent())?;
        create_parent_dir_if_present(config.url_list_json_file_path.parent())?;
    }

    let mut json_file = File::create(&config.modified_json_file_path)
        .chain_err(|| "Unable to open modified update-center file for writing")?;

    let serialized_json = serde_json::to_string(&resp_json)
        .chain_err(|| "Unable to convert modified JSON back into string for serialization")?;

    // need to append back the trimmed left and right sides

    json_file
        .write_fmt(format_args!(
            "{}{}{}",
            config.suppress_front,
            serialized_json,
            config.suppress_back
        ))
        .chain_err(|| "Unable to write modified serialized JSON to file")?;

    let mut urls_file = File::create(&config.url_list_json_file_path)
        .chain_err(|| "Unable to open file for writing URLs")?;

    let urls_json = serde_json::to_string_pretty(&urls)
        .chain_err(|| "Unable to convert list of URLs into pretty JSON form")?;

    urls_file
        .write_fmt(format_args!("{}", urls_json))
        .chain_err(|| "Unable to write URLs in JSON form into file")?;

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
