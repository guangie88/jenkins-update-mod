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
extern crate url;

use hyper::client::{Client, RedirectPolicy};
use serde_json::{Map, Value};
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process;
use structopt::StructOpt;
use url::Url;

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

    let read_from_file_into_string = |path: &Path| {
        let mut file = File::open(path)
            .chain_err(|| format!("Unable to open file path at {:?}", path))?;

        let mut s = String::new();

        file.read_to_string(&mut s)
            .map(|_| s)
            .chain_err(|| "Unable to read file into string")
    };

    let config_str = read_from_file_into_string(Path::new(&arg_config.config_path))?;

    let config: FileConfig = toml::from_str(&config_str)
        .chain_err(|| format!("Unable to parse config as required toml format: {}", config_str))?;

    info!("Completed configuration initialization!");

    // write the body here
    let urls_str = read_from_file_into_string(&config.url_list_json_file_path)?;

    let urls: Vec<String> = serde_json::from_str(&urls_str)
        .chain_err(|| format!("Error in parsing URL list from {:?}", config.url_list_json_file_path))?;

    let urls: Vec<_> = urls.into_iter()
        .map(|url| Url::parse(&url))
        .inspect(|url_res| {
            // log any erroneous URL and continue
            if let &Err(e) = url_res {
                error!("Unable to parse into URL: {}", e);
            }
        })
        .filter_map(|url_res| url_res.ok())
        .collect();

    let url_paths: Vec<_> = urls.iter()
        .map(|url| format!("{}{}", config.sync_root_dir_path, url.path()))
        .collect();

    let mut json_file = File::create("url_paths.json")
        .chain_err(|| "Unable to open modified update-center file for writing")?;

    let serialized_json = serde_json::to_string_pretty(&url_paths)
        .chain_err(|| "Unable to convert modified JSON back into string for serialization")?;

    json_file.write_fmt(format_args!("{}", serialized_json))
        .chain_err(|| "Unable to write modified serialized JSON to file")?;
    
//     let mut client = Client::new();
//     client.set_redirect_policy(RedirectPolicy::FollowAll);
// 
//     let mut resp = client.get(&config.update_center_url).send()
//         .chain_err(|| format!("Unable to perform HTTP request with URL string '{}'", config.update_center_url))?;
// 
//     let mut resp_str = String::new();
//     resp.read_to_string(&mut resp_str)
//         .chain_err(|| "Unable to read HTTP response into string")?;
// 
//     let resp_str = resp_str;
// 
//     let trimmed_resp_str = resp_str
//         .trim_left_matches(&config.suppress_front)
//         .trim_right_matches(&config.suppress_back);
// 
//     // JSON parsing all the way
//     let mut resp_json: Value = serde_json::from_str(trimmed_resp_str)
//         .chain_err(|| "Unable to parse trimmed JSON string into JSON value.")?;
// 
//     // to stop borrowing early
//     let (core_orig_url, mut plugin_urls) = {
//         let mut resp_outer_map = match resp_json {
//             Value::Object(ref mut resp_outer_map) => resp_outer_map,
//             c @ _ => bail!(format!("Expected outer most JSON to be of Object type, but found content: {:?}", c)),
//         };
// 
//         change_connection_check_url(&mut resp_outer_map, config.connection_check_url_change.to_owned())?;
//         let core_orig_url = replace_core_url(&mut resp_outer_map, &config.url_replace_from, &config.url_replace_into)?;
//         let plugin_urls = replace_plugin_urls(&mut resp_outer_map, &config.url_replace_from, &config.url_replace_into)?;
// 
//         (core_orig_url, plugin_urls)
//     };
// 
//     // combine both the core + plugin links
//     let mut urls = vec![core_orig_url];
//     urls.append(&mut plugin_urls);
//     let urls = urls;
// 
//     // write the modified JSON file
//     if config.auto_create_output_dir {
//         let create_parent_dir_if_present = |dir_opt: Option<&Path>| {
//             let dir_opt = dir_opt.and_then(|dir| {
//                 // ignore if the directory has already been created
//                 match Path::new(dir).is_dir() {
//                     true => None,
//                     false => Some(dir),
//                 }
//             });
// 
//             match dir_opt {
//                 Some(dir) => {
//                     info!("Creating directory chain: {:?}", dir);
// 
//                     fs::create_dir_all(dir)
//                         .chain_err(|| format!("Unable to create directory chain: {:?}", dir))
//                 },
// 
//                 None => Ok(())
//             }
//         };
// 
//         create_parent_dir_if_present(config.modified_json_file_path.parent())?;
//         create_parent_dir_if_present(config.url_list_json_file_path.parent())?;
//     }
//     
//     let mut json_file = File::create(&config.modified_json_file_path)
//         .chain_err(|| "Unable to open modified update-center file for writing")?;
// 
//     let serialized_json = serde_json::to_string(&resp_json)
//         .chain_err(|| "Unable to convert modified JSON back into string for serialization")?;
// 
//     json_file.write_fmt(format_args!("{}", serialized_json))
//         .chain_err(|| "Unable to write modified serialized JSON to file")?;
// 
//     let mut urls_file = File::create(&config.url_list_json_file_path)
//         .chain_err(|| "Unable to open file for writing URLs")?;
// 
//     let urls_json = serde_json::to_string_pretty(&urls)
//         .chain_err(|| "Unable to convert list of URLs into pretty JSON form")?;
// 
//     urls_file.write_fmt(format_args!("{}", urls_json))
//         .chain_err(|| "Unable to write URLs in JSON form into file")?;

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
