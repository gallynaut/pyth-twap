use crate::config;
use pyth_client::{AccountType, Mapping, Price, Product, MAGIC, VERSION_2};
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
    pub twap: i64,
}

pub fn get_price_account(c: &config::Config) -> Result<PriceAccount, &'static str> {
    // read pyth_map_key account data and verify it is the correct account
    // mapping accounts stored as linked list so we iterate until empty
    let mut akey = Pubkey::from_str(&c.pyth_key).unwrap();

    loop {
        let map_data = c.rpc_client.get_account_data(&akey).unwrap();
        let map_acct = cast::<Mapping>(&map_data).unwrap();
        if !valid_mapping_account(&map_acct) {
            panic!("not a valid pyth mapping account");
        }
        println!("mapping_account .. {:?}", akey);

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
            if !valid_product_account(&prod_acct) {
                continue;
            }

            // loop through reference attributes and find symbol
            let prod_attr_sym = get_attr_symbol(prod_acct);
            if prod_attr_sym != c.symbol {
                i += 1;
                if i == map_acct.num {
                    break;
                }
                if c.debug {
                    println!("symbols do not match {} v {}", c.symbol, prod_attr_sym);
                }
                continue;
            }
            println!("product_account .. {:?}", prod_pkey);

            if !prod_acct.px_acc.is_valid() {
                println!("pyth error: price account is invalid");
                return Err("pyth error: price account is invalid");
            }

            // check if price account is valid
            let mut price_pkey = Pubkey::new(&prod_acct.px_acc.val);
            let mut p: &Price;
            loop {
                let price_data = c.rpc_client.get_account_data(&price_pkey);
                let price_data = match price_data {
                    Ok(price_acct) => price_acct,
                    Err(_) => return Err("errorrrrrrr"), // go to next loop if no product account
                };
                p = cast::<Price>(&price_data).unwrap();
                if !valid_price_account(&p) {
                    return Err("pyth error: price account is invalid");
                }

                if valid_price_account_type(&p, "price") {
                    return Ok(PriceAccount {
                        key: price_pkey,
                        expo: p.expo,
                        twap: p.twap,
                    });
                }
                // go to next Mapping account in list
                if !p.next.is_valid() {
                    return Err("price account not found");
                }
                if c.debug {
                    println!("going to next price account");
                }
                price_pkey = Pubkey::new(&p.next.val);
                continue;
            }
        }
        // go to next Mapping account in list
        if !map_acct.next.is_valid() {
            break;
        }
        if c.debug {
            println!("going to next mapping account");
        }
        akey = Pubkey::new(&map_acct.next.val);
    }
    println!(
        "See {} for a list of symbols",
        "https://pyth.network/markets/"
    );
    return Err("product account not found");
}

pub fn cast<T>(d: &[u8]) -> Option<&T> {
    let (_, pxa, _) = unsafe { d.align_to::<T>() };
    if pxa.len() > 0 {
        Some(&pxa[0])
    } else {
        None
    }
}

fn valid_mapping_account(acct: &Mapping) -> bool {
    if acct.magic != MAGIC || acct.atype != AccountType::Mapping as u32 || acct.ver != VERSION_2 {
        return false;
    }
    true
}
fn valid_product_account(acct: &Product) -> bool {
    if acct.magic != MAGIC || acct.atype != AccountType::Product as u32 || acct.ver != VERSION_2 {
        return false;
    }
    true
}
fn valid_price_account(acct: &Price) -> bool {
    if acct.magic != MAGIC || acct.atype != AccountType::Price as u32 || acct.ver != VERSION_2 {
        return false;
    }
    true
}
fn valid_price_account_type(acct: &Price, ptype: &str) -> bool {
    let acct_ptype = match &acct.ptype {
        PriceType::Unknown => "unknown",
        PriceType::Price => "price",
    };
    if acct_ptype != ptype {
        return false;
    }
    true
}
pub fn valid_price_instruction(instr: &UpdatePriceData) -> bool {
    let instr_status = match &instr.status {
        PriceStatus::Trading => "trading",
        _ => "unknown",
    };
    if instr_status != "trading" {
        return false;
    }
    true
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
