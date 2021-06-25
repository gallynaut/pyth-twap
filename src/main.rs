mod config;
mod pyth;
use chrono::prelude::DateTime;
use chrono::Utc;
use progress_bar::color::{Color, Style};
use progress_bar::progress_bar::ProgressBar;
use solana_client::rpc_client::GetConfirmedSignaturesForAddress2Config;
use solana_sdk::signature::Signature;
use solana_transaction_status::UiTransactionEncoding;
use std::collections::HashMap;
use std::process;
use std::str::FromStr;
use std::time::{Duration as StdDuration, UNIX_EPOCH};

fn main() {
    let c = config::Config::new().unwrap_or_else(|err| {
        println!("Config Err: {:?}", err);
        process::exit(1);
    });

    let p = match pyth::get_price_account(&c) {
        Ok(price_account) => price_account,
        Err(error) => {
            panic!("Pyth Err: {:?}", error)
        }
    };
    println!("price_account .. {:?}", p.key);

    println!("");
    println!("Parsing price account transactions");
    let mut progress_bar = ProgressBar::new(100);
    progress_bar.set_action(" Progress", Color::Blue, Style::Bold);

    // Loop through transactions and get last N transactions over the given interval
    let start = Utc::now();
    let mut last_sig: Option<Signature> = None;

    // do we even need to store this?
    // https://uniswap.org/docs/v2/core-concepts/oracles/
    let mut map = HashMap::new(); // <slot, price>
    let mut high: i64 = 0;
    let mut low: i64 = std::i64::MAX;
    let mut open_slot: u64 = std::u64::MAX;
    let mut close_slot: u64 = 0;
    'process_px_acct: loop {
        let rqt_config = GetConfirmedSignaturesForAddress2Config {
            before: last_sig,
            until: None,
            limit: None,
            commitment: None,
        };
        if c.debug {
            println!("getting next batch of transactions");
        }

        let px_sigs = c
            .rpc_client
            .get_signatures_for_address_with_config(&p.key, rqt_config);
        let px_sig_rslt = px_sigs.unwrap();
        for sig in px_sig_rslt {
            // check for signature error
            let e = sig.err;
            match e {
                Some(_) => continue,
                None => (),
            }
            // check time duration
            let block_t = sig.block_time.unwrap() as u64;
            let block_t = UNIX_EPOCH + StdDuration::from_secs(block_t);
            let block_t = DateTime::<Utc>::from(block_t);
            if (start - block_t) > c.interval {
                if c.debug {
                    println!("interval exceeded, breaking out of loop");
                }
                break 'process_px_acct;
            }
            // request transaction from signature
            let s = Signature::from_str(&sig.signature).unwrap();
            last_sig = Some(s);
            let txn = c
                .rpc_client
                .get_transaction(&s, UiTransactionEncoding::Base64)
                .unwrap();
            let t = txn.transaction.transaction.decode().unwrap(); // transaction
            let instrs = t.message.instructions;
            let i = &instrs.first().unwrap(); // first instruction
            let d = &i.data;
            let data = pyth::cast::<pyth::UpdatePriceData>(&d);
            match data {
                None => continue,
                _ => (),
            }
            let data = data.unwrap();
            // check if empty price
            if data.price == 0 {
                continue;
            }

            if c.debug {
                println!("{}: p: {}, c: {}", data.pub_slot, data.price, data.conf);
            }

            if data.price < low {
                low = data.price
            }
            if data.price > high {
                high = data.price
            }
            if data.pub_slot < open_slot {
                open_slot = data.pub_slot
            }
            if data.pub_slot > close_slot {
                close_slot = data.pub_slot
            }
            // insert into hashmap. Updates price if pub_slot exist
            // should be comparing confidence value but data is inconsistent
            map.insert(data.pub_slot, data.price);

            // update progress bar
            let progress_microseconds = (start - block_t).num_microseconds().unwrap();
            let interval_microseconds = c.interval.num_microseconds().unwrap();
            let time_progress =
                (100.0 * progress_microseconds as f32) / (interval_microseconds as f32);
            progress_bar.set_progression(time_progress as usize);
        }
    }

    // calculate twap using first and last value over accrued interval
    let open = map.get(&open_slot).unwrap();
    let close = map.get(&close_slot).unwrap();

    let base: f32 = 10.0;
    let scale_factor: f32 = base.powi(p.expo);
    let open_price = (*open as f32) * scale_factor;
    let close_price = (*close as f32) * scale_factor;
    let low_price = (low as f32) * scale_factor;
    let high_price = (high as f32) * scale_factor;
    let twap_price = (open_price + close_price + low_price + high_price) / 4.0;

    println!("");
    println!("TWAP Interval: {} minutes", c.interval.num_minutes());
    println!("Open: ${} ({})", open_price, open_slot);
    println!("High: ${}", high_price);
    println!("Low: ${}", low_price);
    println!("Close: ${} ({})", close_price, close_slot);
    println!("TWAP Price: ${}", twap_price);
}
