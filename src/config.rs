use chrono::Duration;
use clap::{App, Arg};
use solana_client::rpc_client::RpcClient;

pub struct Config {
    pub symbol: String,
    pub interval: Duration,
    pub pyth_key: String,
    pub debug: bool,
    pub rpc_client: RpcClient,
}

impl Config {
    pub fn new() -> Result<Config, &'static str> {
        // validate command line arguements
        let matches = App::new("Pyth-TWAP")
            .version("0.1.0")
            .author("Conner <ConnerNGallagher@gmail.com>")
            .about("using pyth price oracle to calculate twap")
            .arg(
                Arg::with_name("symbol")
                    .help("the symbol to calculate the TWAP for (BTC/USD)")
                    .index(1)
                    .required(true),
            )
            .arg(
                Arg::with_name("debug")
                    .help("print debug information verbosely")
                    .short("d"),
            )
            .arg(
                Arg::with_name("local")
                    .short("l")
                    .help("run on a local instance of solana (http://localhost:8899)"),
            )
            .arg(
                Arg::with_name("pyth")
                    .short("p")
                    .help("sets the public key of the pyth mapping account")
                    .takes_value(true)
                    .default_value("BmA9Z6FjioHJPpjT39QazZyhDRUdZy2ezwx4GiDdE2u2")
                    .required(false),
            )
            .arg(
                Arg::with_name("interval")
                    .short("i")
                    .help("the interval to calculate the TWAP over in minutes")
                    .takes_value(true)
                    .default_value("60")
                    .required(false),
            )
            .get_matches();

        let symbol = matches.value_of("symbol").unwrap().to_string();
        println!("{:.<20} {}", "symbol", symbol);

        let interval = matches
            .value_of("interval")
            .unwrap()
            .parse::<i64>()
            .unwrap();
        if interval == 0 || interval > 1440 {
            // panic
            return Err("interval should be between 1 and 1440 minutes (1 day)");
        }
        let interval = Duration::seconds(interval.checked_mul(60).unwrap());
        println!(
            "{:.<20} {} minute(s)",
            "TWAP interval",
            interval.num_minutes()
        );

        let pyth_key = matches.value_of("pyth").unwrap().to_string();

        let mut url = "http://api.devnet.solana.com";
        if matches.is_present("local") {
            url = "http://localhost";
        }
        let debug = matches.is_present("debug");

        println!("{:.<20} {}", "Solana RPC Url", url);
        let rpc_client = RpcClient::new(url.to_string());

        Ok(Config {
            symbol,
            interval,
            pyth_key,
            debug,
            rpc_client,
        })
    }
}
