use chrono::{Duration, TimeZone, Utc};
use clap::{App, Arg};
use pyth_client::{
    AccountType, Mapping, Price, PriceStatus, PriceType, Product, MAGIC, PROD_HDR_SIZE, VERSION_1,
};
use solana_client::rpc_client::{GetConfirmedSignaturesForAddress2Config, RpcClient};
use solana_client::rpc_response::RpcConfirmedTransactionStatusWithSignature;
use solana_program::pubkey::Pubkey;
use solana_sdk::signature::Signature;
use solana_transaction_status::UiTransactionEncoding;
use std::str;
use std::str::FromStr;

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
                .help("the interval to calculate the TWAP over in hours")
                .takes_value(true)
                .default_value("24")
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
    if interval < 0 && interval > 168 * 3600 {
        // panic
        println!("interval should be between 0 and 168 hours");
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
            let prod_acct = cast::<Product>(&prod_data).unwrap();
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
                continue;
            }

            if prod_acct.px_acc.is_valid() {
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
                println!("success:");

                // Loop through transactions and get last N transactions over the given interval in hours
                let now = Utc::now();
                let mut last_sig: Option<Signature> = None;

                let mut signatures: Vec<RpcConfirmedTransactionStatusWithSignature> = Vec::new();
                loop {
                    let rqt_config = GetConfirmedSignaturesForAddress2Config {
                        before: last_sig,
                        until: None,
                        limit: None,
                        commitment: None,
                    };
                    let px_txs =
                        rpc_client.get_signatures_for_address_with_config(&px_pkey, rqt_config);
                    let mut px_txs_rslt = px_txs.unwrap();

                    let last = px_txs_rslt.last().unwrap();
                    let t = Utc.timestamp(last.block_time.unwrap(), 0);
                    let d: Duration = now - t;
                    println!("{} minutes have passed", (d.num_seconds() / 60) % 60);

                    // set last config to reloop
                    let sig = Signature::from_str(&last.signature).unwrap();
                    last_sig = Some(sig);

                    signatures.append(&mut px_txs_rslt);
                    if (now - t) > interval {
                        break;
                    }
                }
                println!("{}", signatures.first().unwrap().block_time.unwrap());
                let _rslt = calculate_twap(rpc_client, signatures, interval);
                return;
            }
            // go to next product
            i += 1;
            if i == map_acct.num {
                break;
            }
        }

        // go to next Mapping account in list
        if !map_acct.next.is_valid() {
            break;
        }
        akey = Pubkey::new(&map_acct.next.val);
    }
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

fn calculate_twap(
    client: RpcClient,
    signatures: Vec<RpcConfirmedTransactionStatusWithSignature>,
    interval: Duration,
) -> u64 {
    for sig in signatures {
        let e = sig.err;
        match e {
            Some(_) => continue,
            None => println!(""),
        }
        let s = Signature::from_str(&sig.signature).unwrap();
        println!("sig: {}", s);
        let txn = client
            .get_transaction(&s, UiTransactionEncoding::Base64)
            .unwrap();
        // deserialize into struct
        // let enc: EncodedConfirmedTransaction = serde_json::from_str(&txn).unwrap();

        let t = txn.transaction.transaction;
        let t = t.decode().unwrap();
        let i = t.message.instructions.first().unwrap();
        let d = &i.data;
        println!("raw: {:?}", d);
        let data = cast::<UpdatePriceData>(&d);
        match data {
            Some(d) => println!("p: {:?}, conf: {:?}", d.price, d.conf),
            None => continue,
        }
    }

    return 0;
}

pub fn cast<T>(d: &[u8]) -> Option<&T> {
    let (_, pxa, _) = unsafe { d.align_to::<T>() };
    if pxa.len() > 0 {
        Some(&pxa[0])
    } else {
        None
    }
}
