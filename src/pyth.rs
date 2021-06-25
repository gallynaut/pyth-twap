use crate::config;
use pyth_client::{AccountType, Mapping, Price, Product, MAGIC, VERSION_1};
use pyth_client::{PriceStatus, PriceType, PROD_HDR_SIZE};
use solana_program::pubkey::Pubkey;
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

pub struct PriceAccount {
    pub key: Pubkey,
    pub expo: i32,
}

pub fn get_attr_str<'a, T>(ite: &mut T) -> String
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
pub fn get_attr_symbol(prod_acct: &Product) -> String {
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
pub fn get_price_type(ptype: &PriceType) -> &'static str {
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

pub fn get_price_account(c: &config::Config) -> Result<PriceAccount, &'static str> {
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
                return Err("pyth error: price account is invalid");
            }

            // check if price account is valid
            let px_pkey = Pubkey::new(&prod_acct.px_acc.val);
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

            return Ok(PriceAccount {
                key: px_pkey,
                expo: pa.expo,
            });
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
    return Err("price account not found");
}

// pub fn get_price_data(data: [u8]) -> Result<Price, &'static str> {
//     let pd = c.rpc_client.get_account_data(&px_pkey).unwrap();
//     let pa = cast::<Price>(&pd).unwrap();
//     assert_eq!(pa.magic, MAGIC, "not a valid pyth account");
//     assert_eq!(
//         pa.atype,
//         AccountType::Price as u32,
//         "not a valid pyth price account"
//     );
//     assert_eq!(pa.ver, VERSION_1, "unexpected pyth price account version");

//     // price accounts are stored as linked list
//     // if first acct type doesnt equal price then panic
//     assert_eq!(
//         get_price_type(&pa.ptype),
//         "price",
//         "couldnt find price account with type price"
//     );
//     Ok(pa)
// }
