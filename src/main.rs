#[macro_use]
extern crate error_chain;
extern crate hyper;

#[macro_use]
extern crate log;
extern crate log4rs;

#[macro_use]
extern crate serde_derive;
extern crate structopt;

#[macro_use]
extern crate structopt_derive;
extern crate toml;

use hyper::client::{Client, RedirectPolicy};
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
}

#[derive(StructOpt, Debug)]
#[structopt(name = "Test", about = "Test program")]
struct ArgConfig {
    #[structopt(short = "c", long = "config", help = "File configuration path")]
    config_path: String,

    #[structopt(short = "l", long = "log-config", help = "Log configuration file path")]
    log_config_path: String,
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

    let mut json_file = File::create("update-center.json")
        .chain_err(|| "Unable to open file for writing")?;

    json_file.write_fmt(format_args!("{}", resp_str))
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
