use colored::Colorize;
use futures::future;
use futures::{stream, StreamExt};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json;
use std::fs::File;
use std::io;
use std::io::prelude::*;
use std::io::BufReader;
use std::net::IpAddr;
use structopt::StructOpt;
use tokio;

const CONCURRENCY: usize = 10;
const EXIT_ERROR_CODE: i32 = 1;

#[derive(Deserialize, Serialize, Debug)]
struct Host {
    cpes: Vec<String>,
    hostnames: Vec<String>,
    ip: String,
    ports: Vec<u16>,
    tags: Vec<String>,
    vulns: Vec<String>,
}

#[derive(Debug, StructOpt)]
#[structopt(name = "nrich", about = "Add network information to IPs")]
struct Cli {
    /// Output format (shell or json)
    #[structopt(default_value = "shell", short, long)]
    output: String,

    /// File containing an IP per line. Non-IPs are ignored.
    filename: String,
}

#[tokio::main]
async fn main() {
    let args = Cli::from_args();

    let input: Box<dyn io::Read> = match args.filename == "-" {
        true => Box::new(io::stdin()),
        _ => {
            let file = File::open(args.filename);
            if let Err(e) = file {
                println!("{}: {}", "Error".red(), e);
                std::process::exit(EXIT_ERROR_CODE);
            }
            Box::new(file.unwrap())
        }
    };
    let reader = BufReader::new(input);
    let client = Client::new();

    let ip_lookups = stream::iter(reader.lines())
        // We only care about IP addresses
        .filter(|line| match line {
            Ok(line) => future::ready(line.parse::<IpAddr>().is_ok()),
            Err(_) => future::ready(false),
        })
        // Do the IP lookup in InternetDB
        .map(|line| {
            let client = &client;
            async move {
                let url = format!("https://internetdb.shodan.io/{}", line.unwrap());
                let response = client.get(url).send().await;

                // If we can't connect to the API then error out
                if let Err(e) = response {
                    println!("{}: {}", "Error".red(), e);
                    std::process::exit(EXIT_ERROR_CODE);
                }

                response.unwrap().json::<Host>().await
            }
        })
        .buffer_unordered(CONCURRENCY);

    ip_lookups
        .for_each(|result| async {
            // We got some information from InternetDB
            if let Ok(host) = result {
                if args.output == "json" {
                    println!("{}", serde_json::to_string(&host).unwrap());
                } else {
                    // Terminal output should look something like this
                    //
                    // 1.1.1.1 (one.one.one.one)
                    //   Ports: 53, 443
                    //   Vulnerabilities: CVE-2014-0160
                    print!("{}", host.ip.white().bold());
                    if !host.hostnames.is_empty() {
                        print!(" ({})", host.hostnames.join(", "));
                    }
                    print!("\n");

                    if host.ports.len() > 0 {
                        println!(
                            "  Ports: {}",
                            host.ports
                                .iter()
                                .map(|p| p.to_string().green().to_string())
                                .collect::<Vec<String>>()
                                .join(", ")
                        );
                    }
                    if host.tags.len() > 0 {
                        println!(
                            "  Tags: {}",
                            host.tags
                                .iter()
                                .map(|p| p.blue().to_string())
                                .collect::<Vec<String>>()
                                .join(", ")
                        );
                    }
                    if host.cpes.len() > 0 {
                        println!(
                            "  CPEs: {}",
                            host.cpes
                                .iter()
                                .map(|p| p.yellow().to_string())
                                .collect::<Vec<String>>()
                                .join(", ")
                        );
                    }
                    if host.vulns.len() > 0 {
                        println!(
                            "  Vulnerabilities: {}",
                            host.vulns
                                .iter()
                                .map(|p| p.red().to_string())
                                .collect::<Vec<String>>()
                                .join(", ")
                        );
                    }

                    print!("\n");
                }
            }
        })
        .await;
}
