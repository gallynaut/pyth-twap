use chrono::prelude::DateTime;
use chrono::{Duration, Utc};
use clap::{App, Arg};
use pyth_client::{
    AccountType, Mapping, Price, PriceStatus, PriceType, Product, MAGIC, PROD_HDR_SIZE, VERSION_1,
};
use solana_client::rpc_client::{GetConfirmedSignaturesForAddress2Config, RpcClient};
use solana_program::pubkey::Pubkey;
use solana_sdk::signature::Signature;
use solana_transaction_status::UiTransactionEncoding;
use std::collections::HashMap;
use std::str;
use std::str::FromStr;
use std::time::{Duration as StdDuration, UNIX_EPOCH};

#[derive(Debug, Clone, Copy)]
pub struct PriceFeed {
    pub price: i64,
    pub conf: u64,
    pub pub_slot: u64,
    pub time: DateTime<Utc>,
    pub twap: f64,
}
impl Default for PriceFeed {
    fn default() -> Self {
        PriceFeed {
            price: 0,
            conf: 0,
            pub_slot: 0,
            time: Utc::now(),
            twap: 0.0,
        }
    }
}

#[repr(C)]
pub struct UpdatePriceData {
    pub version: u32,
    pub cmd: i32,
    pub status: PriceStatus,
    pub unused: u32,
    pub price: i64,
    pub conf: u64,
    pub pub_slot: u64,
}

fn main() {
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
            Arg::with_name("local")
                .short("l")
                .help("run on a local instance of solana (http://localhost:8899)"),
        )
        .arg(
            Arg::with_name("pyth")
                .short("p")
                .help("sets the public key of the pyth mapping account")
                .takes_value(true)
                .default_value("ArppEFcsybCLE8CRtQJLQ9tLv2peGmQoKWFuiUWm4KBP")
                .required(false),
        )
        .arg(
            Arg::with_name("interval")
                .short("i")
                .help("the interval to calculate the TWAP over in minutes")
                .takes_value(true)
                .default_value("15")
                .required(false),
        )
        .get_matches();

    let symbol = matches.value_of("symbol").unwrap();
    println!("Value for symbol: {}", symbol);

    let interval = matches
        .value_of("interval")
        .unwrap()
        .parse::<i64>()
        .unwrap()
        .checked_mul(60) ////////
        .unwrap();
    if interval < 0 && interval > 1440 * 60 {
        // panic
        println!("interval should be between 0 and 1440 minutes");
        return;
    }
    let interval = Duration::seconds(interval);
    println!("Value for interval: {:?}", interval);

    let pyth_map_key = matches.value_of("pyth").unwrap();
    println!("using new pyth map key: {}", pyth_map_key);

    let mut url = "http://api.devnet.solana.com";
    if matches.is_present("local") {
        url = "http://localhost";
    }

    println!("Connecting to Solana @: {}", url);
    let rpc_client = RpcClient::new(url.to_string());

    ////////////////////////////////////////////

    // read pyth_map_key account data and verify it is the correct account
    // mapping accounts stored as linked list so we iterate until empty
    let mut akey = Pubkey::from_str(pyth_map_key).unwrap();

    loop {
        let map_data = rpc_client.get_account_data(&akey).unwrap();
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
            let prod_data = rpc_client.get_account_data(&prod_pkey).unwrap();
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
            // println!("product_account .. {:?}", prod_pkey);
            let pr_attr_sym = get_attr_symbol(prod_acct);
            if pr_attr_sym != symbol {
                i += 1;
                if i == map_acct.num {
                    break;
                }
                println!("symbols do not match {} v {}", symbol, pr_attr_sym);
                continue;
            }

            if !prod_acct.px_acc.is_valid() {
                println!("pyth error: price account is invalid");
                return;
            }
            let px_pkey = Pubkey::new(&prod_acct.px_acc.val);
            println!("price_account .. {:?}", px_pkey);

            let pd = rpc_client.get_account_data(&px_pkey).unwrap();
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

            // Loop through transactions and get last N transactions over the given interval in hours
            let start = Utc::now();
            let mut last_sig: Option<Signature> = None;

            // do we even need to store this?
            // https://uniswap.org/docs/v2/core-concepts/oracles/
            let mut price_feed: Vec<PriceFeed> = Vec::new();
            let mut high: u64;
            let mut low: u64;
            let mut open: u64;
            let mut close: u64;
            'process_px_acct: loop {
                let rqt_config = GetConfirmedSignaturesForAddress2Config {
                    before: last_sig,
                    until: None,
                    limit: None,
                    commitment: None,
                };
                println!("getting next batch of transactions");
                let px_sigs =
                    rpc_client.get_signatures_for_address_with_config(&px_pkey, rqt_config);
                let px_sig_rslt = px_sigs.unwrap();
                for sig in px_sig_rslt {
                    let mut pf = PriceFeed::default();
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
                    if (start - block_t) > interval {
                        println!("interval exceeded, breaking out of loop");
                        break 'process_px_acct;
                    }
                    // request transaction from signature
                    let s = Signature::from_str(&sig.signature).unwrap();
                    last_sig = Some(s);
                    let txn = rpc_client
                        .get_transaction(&s, UiTransactionEncoding::Base64)
                        .unwrap();
                    let t = txn.transaction.transaction.decode().unwrap(); // transaction
                    let instrs = t.message.instructions;
                    let i = &instrs.first().unwrap(); // first instruction
                    let d = &i.data; // data
                                     // cast data to pyth object
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

                    pf.price = data.price;
                    pf.conf = data.conf;
                    pf.pub_slot = data.pub_slot;
                    pf.time = block_t;
                    println!("{}: p: {}, c: {}", pf.pub_slot, pf.price, pf.conf);
                    // compare previous result price/conf then add to vec
                    if price_feed.len() == 0 {
                        pf.twap = 0.0;
                        price_feed.push(pf);
                        continue;
                    }
                    let last = price_feed.last();
                    let mut last = match last {
                        Some(last) => last,
                        None => continue, // ?????????
                    };
                    if last.pub_slot == pf.pub_slot {
                        if last.conf < pf.conf {
                            // println!("overwriting data because confidence is lower");
                            price_feed.remove(price_feed.len() - 1);
                            let new_last = price_feed.last();
                            last = match new_last {
                                Some(new_last) => new_last,
                                None => continue,
                            };
                            // last = price_feed.last().unwrap();
                        } else {
                            // println!("confidence is higher, skipping");
                            continue;
                        }
                    }
                    let dur = (last.time - pf.time).num_nanoseconds().unwrap();
                    // println!("{} ----> {} ({})", last.time, pf.time, dur);
                    price_feed.push(pf);

                    // pf.twap = (last.time -
                }
                // reached end of loop but not done yet
                // let last_s = px_sig_rslt.last().unwrap();
                // let last_s = Signature::from_str(last_s.signature);
                // last_sig = Some(last_s);
            }

            // calculate twap using first and last value over accrued interval

            // let _rslt = calculate_twap(rpc_client, signatures, interval);
            return;
        }
        // go to next Mapping account in list
        if !map_acct.next.is_valid() {
            break;
        }
        akey = Pubkey::new(&map_acct.next.val);
    }
    println!("No matching symbol found for {}", symbol);
    return;
}

fn get_attr_str<'a, T>(ite: &mut T) -> String
where
    T: Iterator<Item = &'a u8>,
{
    let mut len = *ite.next().unwrap() as usize;
    let mut val = String::with_capacity(len);
    while len > 0 {
        val.push(*ite.next().unwrap() as char);
        len -= 1;
    }
    return val;
}

// loops through a products reference data (key/val) and
// returns the value for symbol
fn get_attr_symbol(prod_acct: &Product) -> String {
    let mut pr_attr_sz = prod_acct.size as usize - PROD_HDR_SIZE;
    let mut pr_attr_it = (&prod_acct.attr[..]).iter();
    while pr_attr_sz > 0 {
        let key = get_attr_str(&mut pr_attr_it);
        let val = get_attr_str(&mut pr_attr_it);
        // println!("  {:.<16} {}", key, val);
        if key == "symbol" {
            return val.to_string();
        }
        pr_attr_sz -= 2 + key.len() + val.len();
    }
    return "".to_string();
}
fn get_price_type(ptype: &PriceType) -> &'static str {
    match ptype {
        PriceType::Unknown => "unknown",
        PriceType::Price => "price",
        PriceType::TWAP => "twap",
        PriceType::Volatility => "volatility",
    }
}

pub fn cast<T>(d: &[u8]) -> Option<&T> {
    let (_, pxa, _) = unsafe { d.align_to::<T>() };
    if pxa.len() > 0 {
        Some(&pxa[0])
    } else {
        None
    }
}