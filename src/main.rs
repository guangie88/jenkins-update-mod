#[macro_use]
extern crate error_chain;
extern crate hyper;

#[macro_use]
extern crate log;
extern crate log4rs;

#[macro_use]
extern crate serde_derive;
extern crate serde_json;
extern crate structopt;

#[macro_use]
extern crate structopt_derive;
extern crate toml;

use hyper::client::{Client, RedirectPolicy};
use serde_json::{Map, Value};
use std::fs::File;
use std::io::{self, Read, Write};
use std::process;
use structopt::StructOpt;

mod errors {
    error_chain! {
        errors {
            // ClientMapRead {
            //     description("error in reading client map")
            //     display("error in reading client map")
            // }
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
const CONNECTION_CHECK_URL_KEY: &'static str = "connectionCheckUrl";
const CORE_KEY: &'static str = "core";
const PLUGINS_KEY: &'static str = "plugins";
const URL_KEY: &'static str = "url";

type MapStrVal = Map<String, Value>;

fn change_connection_check_url<S: Into<String>>(resp_outer_map: &mut MapStrVal, connection_check_url_change: S) -> Result<()> {
    let connection_check_url = match resp_outer_map.get_mut(CONNECTION_CHECK_URL_KEY) {
        Some(connection_check_url) => connection_check_url,
        None => bail!(format!("Unable to find '{}' for changing connection URL", CONNECTION_CHECK_URL_KEY)),
    };

    let mut connection_check_url = match connection_check_url {
        &mut Value::String(ref mut connection_check_url) => connection_check_url,
        c @ _ => bail!(format!("Expected '{}' to contain string value, but found content: {:?}", CONNECTION_CHECK_URL_KEY, c)),
    };

    *connection_check_url = connection_check_url_change.into();
    Ok(())
}

fn replace_url_impl(url_outer: &mut Value, outer_key: &str, url_replace_from: &str, url_replace_into: &str) -> Result<()> {
    let mut url_outer_map = match url_outer {
        &mut Value::Object(ref mut url_outer_map) => url_outer_map,
        c @ _ => bail!(format!("Expected '{}' to be an object, but found content: {:?}", outer_key, c)),
    };

    let mut url = match url_outer_map.get_mut(URL_KEY) {
        Some(url) => url,
        None => bail!(format!("Expected '{}' to be present for '{}'", URL_KEY, CORE_KEY)),
    };

    let mut url_str = match url {
        &mut Value::String(ref mut url_str) => url_str,
        c @ _ => bail!(format!("Expected '{}' to contain string value, but found content: {:?}", URL_KEY, c)),
    };

    *url_str = url_str.replace(url_replace_from, url_replace_into);

    Ok(())
}

fn replace_core_url(resp_outer_map: &mut MapStrVal, url_replace_from: &str, url_replace_into: &str) -> Result<()> {
    let mut core = match resp_outer_map.get_mut(CORE_KEY) {
        Some(core) => core,
        None => bail!(format!("Unable to find '{}' for core URL replacement", CORE_KEY)),
    };

    replace_url_impl(&mut core, CORE_KEY, url_replace_from, url_replace_into)?;
    Ok(())
}

fn replace_plugin_urls(resp_outer_map: &mut MapStrVal, url_replace_from: &str, url_replace_into: &str) -> Result<()> {
    Ok(())
}

fn run() -> Result<()> {
    let arg_config = ArgConfig::from_args();

    let _ = log4rs::init_file(&arg_config.log_config_path, Default::default())
       .chain_err(|| format!("Unable to initialize log4rs logger with the given config file at '{}'", arg_config.log_config_path))?;

    let config_str = {
        let mut config_file = File::open(&arg_config.config_path)
            .chain_err(|| format!("Unable to open config file path at {:?}", arg_config.config_path))?;

        let mut s = String::new();

        config_file.read_to_string(&mut s)
            .map(|_| s)
            .chain_err(|| "Unable to read config file into string")?
    };

    let config: FileConfig = toml::from_str(&config_str)
        .chain_err(|| format!("Unable to parse config as required toml format: {}", config_str))?;

    info!("Completed configuration initialization!");

    // write the body here
    let mut client = Client::new();
    client.set_redirect_policy(RedirectPolicy::FollowAll);

    let mut resp = client.get(&config.update_center_url).send()
        .chain_err(|| format!("Unable to perform HTTP request with URL string '{}'", config.update_center_url))?;

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
    {
        let mut resp_outer_map = match resp_json {
            Value::Object(ref mut resp_outer_map) => resp_outer_map,
            c @ _ => bail!(format!("Expected outer most JSON to be of Object type, but found content: {:?}", c)),
        };

        change_connection_check_url(&mut resp_outer_map, config.connection_check_url_change.to_owned())?;
        replace_core_url(&mut resp_outer_map, &config.url_replace_from, &config.url_replace_into)?;
        replace_plugin_urls(&mut resp_outer_map, &config.url_replace_from, &config.url_replace_into)?;
    }

    let mut json_file = File::create("update-center.json")
        .chain_err(|| "Unable to open file for writing")?;

    let serialized_json = serde_json::to_string(&resp_json)
        .chain_err(|| "Unable to convert modified JSON back into string for serialization")?;

    json_file.write_fmt(format_args!("{}", serialized_json))
        .chain_err(|| "Unable to write to file")?;

    Ok(())
}

fn main() {
    match run() {
        Ok(_) => {
            println!("Program completed!");
            process::exit(0)
        },

        Err(ref e) => {
            let stderr = &mut io::stderr();

            writeln!(stderr, "Error: {}", e)
                .expect("Unable to write error into stderr!");

            for e in e.iter().skip(1) {
                writeln!(stderr, "- Caused by: {}", e)
                    .expect("Unable to write error causes into stderr!");
            }

            process::exit(1);
        },
    }
}
