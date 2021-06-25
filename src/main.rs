mod config;
use chrono::prelude::DateTime;
use chrono::Utc;
use progress_bar::color::{Color, Style};
use progress_bar::progress_bar::ProgressBar;
use pyth_client::{AccountType, Mapping, Price, Product, MAGIC, VERSION_1};
use pyth_twap::{cast, get_attr_symbol, get_price_type, UpdatePriceData};
use solana_client::rpc_client::GetConfirmedSignaturesForAddress2Config;
use solana_program::pubkey::Pubkey;
use solana_sdk::signature::Signature;
use solana_transaction_status::UiTransactionEncoding;
use std::collections::HashMap;
use std::process;
use std::str::FromStr;
use std::time::{Duration as StdDuration, UNIX_EPOCH};

fn main() {
    let c = config::Config::new().unwrap_or_else(|err| {
        println!("Config Err: {}", err);
        process::exit(1);
    });

    // read pyth_map_key account data and verify it is the correct account
    // mapping accounts stored as linked list so we iterate until empty
    let mut akey = Pubkey::from_str(&c.pyth_key).unwrap();

    loop {
        let map_data = c.rpc_client.get_account_data(&akey).unwrap();
        let map_acct = cast::<Mapping>(&map_data).unwrap();
        assert_eq!(map_acct.magic, MAGIC, "not a valid pyth account");
        assert_eq!(
            map_acct.atype,
            AccountType::Mapping as u32,
            "not a valid pyth mapping account"
        );
        assert_eq!(
            map_acct.ver, VERSION_1,
            "unexpected pyth mapping account version"
        );

        // loop over products until we find one that matches are symbol
        let mut i = 0;
        for prod_akey in &map_acct.products {
            let prod_pkey = Pubkey::new(&prod_akey.val);
            let prod_data = c.rpc_client.get_account_data(&prod_pkey).unwrap();
            let prod_acct = cast::<Product>(&prod_data);
            let prod_acct = match prod_acct {
                Some(prod_acct) => prod_acct,
                None => continue, // go to next loop if no product account
            };
            assert_eq!(prod_acct.magic, MAGIC, "not a valid pyth account");
            assert_eq!(
                prod_acct.atype,
                AccountType::Product as u32,
                "not a valid pyth product account"
            );
            assert_eq!(
                prod_acct.ver, VERSION_1,
                "unexpected pyth product account version"
            );

            // loop through reference attributes and find symbol
            let pr_attr_sym = get_attr_symbol(prod_acct);
            if pr_attr_sym != c.symbol {
                i += 1;
                if i == map_acct.num {
                    break;
                }
                if c.debug {
                    println!("symbols do not match {} v {}", c.symbol, pr_attr_sym);
                }
                continue;
            }
            println!("product_account .. {:?}", prod_pkey);

            if !prod_acct.px_acc.is_valid() {
                println!("pyth error: price account is invalid");
                return;
            }
            let px_pkey = Pubkey::new(&prod_acct.px_acc.val);
            println!("price_account .. {:?}", px_pkey);

            let pd = c.rpc_client.get_account_data(&px_pkey).unwrap();
            let pa = cast::<Price>(&pd).unwrap();
            assert_eq!(pa.magic, MAGIC, "not a valid pyth account");
            assert_eq!(
                pa.atype,
                AccountType::Price as u32,
                "not a valid pyth price account"
            );
            assert_eq!(pa.ver, VERSION_1, "unexpected pyth price account version");

            // price accounts are stored as linked list
            // if first acct type doesnt equal price then panic
            assert_eq!(
                get_price_type(&pa.ptype),
                "price",
                "couldnt find price account with type price"
            );

            println!("");
            println!("Parsing price account transactions");
            let mut progress_bar = ProgressBar::new(100);
            progress_bar.set_action(" Progress", Color::Blue, Style::Bold);

            // Loop through transactions and get last N transactions over the given interval in hours
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
                    .get_signatures_for_address_with_config(&px_pkey, rqt_config);
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
                    let data = cast::<UpdatePriceData>(&d);
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
            let scale_factor: f32 = base.powi(pa.expo);
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
            return;
        }
        // go to next Mapping account in list
        if !map_acct.next.is_valid() {
            break;
        }
        akey = Pubkey::new(&map_acct.next.val);
    }
    println!("No matching symbol found for {}", c.symbol);
    println!(
        "See {} for a list of symbols",
        "https://pyth.network/markets/"
    );
    return;
}
