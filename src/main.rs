mod config;
mod pyth;
use crate::pyth::{PythAccount, PythClient, UpdatePriceInstruction};
use chrono::prelude::DateTime;
use chrono::Utc;
use progress_bar::color::{Color, Style};
use progress_bar::progress_bar::ProgressBar;
use solana_client::rpc_client::GetConfirmedSignaturesForAddress2Config;
use solana_sdk::signature::Signature;
use solana_transaction_status::UiTransactionEncoding;
use std::process;
use std::str::FromStr;
use std::time::{Duration as StdDuration, UNIX_EPOCH};

fn main() {
    let c = config::Config::new().unwrap_or_else(|err| {
        println!("Config Err: {:?}", err);
        process::exit(1);
    });

    let pyth = PythClient::new(&c.url).unwrap();
    println!("{:.<20} {}", "mapping_account", &c.pyth_key);

    let product_account = match pyth.get_product_account(&c.pyth_key, &c.symbol) {
        Ok(product_account) => product_account,
        Err(error) => {
            println!("Pyth Err: {:?}", error);
            return;
        }
    };
    println!("{:.<20} {}", "product_account", &product_account.key);

    let price_account = match pyth.get_price_account(product_account.price_accounts) {
        Ok(price_account) => price_account,
        Err(error) => {
            println!("Pyth Err: {:?}", error);
            return;
        }
    };
    println!("{:.<20} {}", "price_account", price_account.key);

    println!("");
    println!("Parsing price account transactions");
    let mut progress_bar = ProgressBar::new(100);
    progress_bar.set_action(" Progress", Color::Blue, Style::Bold);

    // Loop through transactions and get last N transactions over the given interval
    let start_t = Utc::now();
    let end_t = start_t - c.interval;
    let interval_microseconds = c.interval.num_microseconds().unwrap();

    // we can request 1000 sig per req
    let mut last_sig: Option<Signature> = None;

    // do we even need to store this?
    // https://uniswap.org/docs/v2/core-concepts/oracles/
    let mut open: Option<i64> = None;
    let mut close: Option<i64> = None;
    let mut high: Option<i64> = None;
    let mut low: Option<i64> = None;
    let mut open_slot: Option<u64> = None;
    let mut close_slot: Option<u64> = None;
    'process_px_acct: loop {
        let rqt_config = GetConfirmedSignaturesForAddress2Config {
            before: last_sig,
            until: None,
            limit: None,
            commitment: None,
        };

        let px_sigs = pyth
            .client
            .get_signatures_for_address_with_config(&price_account.key, rqt_config);
        let price_account_signatures = match px_sigs {
            Ok(result) => result,
            Err(error) => {
                println!("Rpc Err: {}", error);
                continue;
            }
        };
        for sig in price_account_signatures {
            // check for signature error
            if let Some(_) = sig.err {
                if c.debug {
                    println!("{}: Sig Err: {:?}", sig.slot, sig.err.unwrap());
                }
                continue;
            };
            // check time duration
            let block_t = sig.block_time.unwrap() as u64;
            let block_t = UNIX_EPOCH + StdDuration::from_secs(block_t);
            let block_t = DateTime::<Utc>::from(block_t);
            if block_t < end_t {
                progress_bar.set_progression(100);
                break 'process_px_acct;
            }
            // request transaction from signature
            let s = Signature::from_str(&sig.signature).unwrap();
            last_sig = Some(s);
            let txn = pyth
                .client
                .get_transaction(&s, UiTransactionEncoding::Base64)
                .unwrap();
            let t = txn.transaction.transaction.decode().unwrap(); // transaction
            let instrs = t.message.instructions;
            let i = &instrs.first().unwrap(); // first instruction
            let d = &i.data;

            let data = match UpdatePriceInstruction::new::<UpdatePriceInstruction>(&d) {
                None => continue, // skip value
                Some(i) => i,     // unwrap
            };
            // check if empty price or invalid status
            if !data.is_valid() {
                continue;
            }

            if c.debug {
                println!("{}: p: {}, c: {}", data.pub_slot, data.price, data.conf);
            }
            if low == None || data.price < low.unwrap() {
                low = Some(data.price);
            }
            if high == None || data.price > high.unwrap() {
                high = Some(data.price);
            }
            if open_slot == None || data.pub_slot < open_slot.unwrap() {
                open_slot = Some(data.pub_slot);
                open = Some(data.price);
            }
            if close_slot == None || data.pub_slot > close_slot.unwrap() {
                close_slot = Some(data.pub_slot);
                close = Some(data.price);
            }

            // update progress bar
            let progress_microseconds = (start_t - block_t).num_microseconds().unwrap();
            let time_progress =
                (100.0 * progress_microseconds as f32) / (interval_microseconds as f32);
            progress_bar.set_progression(time_progress as usize);
        }
        if c.debug {
            println!("getting next batch of transactions");
        }
    }

    // on a small enough interval there may not be enough data especially with pyth in beta
    if open == None
        || close == None
        || high == None
        || low == None
        || open_slot == None
        || close_slot == None
    {
        println!("Error calculating TWAP - not enough data");
    }

    let base: f32 = 10.0;
    let scale_factor: f32 = base.powi(price_account.expo);
    let open_price = (open.unwrap() as f32) * scale_factor;
    let close_price = (close.unwrap() as f32) * scale_factor;
    let low_price = (low.unwrap() as f32) * scale_factor;
    let high_price = (high.unwrap() as f32) * scale_factor;
    let twap_price = (open_price + close_price + low_price + high_price) / 4.0;

    println!("");
    println!("TWAP Interval: {} minute(s)", c.interval.num_minutes());
    println!("Open: ${} ({})", open_price, open_slot.unwrap());
    println!("High: ${}", high_price);
    println!("Low: ${}", low_price);
    println!("Close: ${} ({})", close_price, close_slot.unwrap());
    println!("Calculated TWAP Price: ${}", twap_price);
    // let pyth_twap_price = (price_account.twap as f32) * scale_factor;
    // println!("Pyth TWAP Price: ${}", pyth_twap_price);
}
