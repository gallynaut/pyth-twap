use pyth_client::{
    AccountType, Mapping, Price, PriceStatus, PriceType, Product, MAGIC, PROD_HDR_SIZE, VERSION_2,
};
use solana_client::rpc_client::RpcClient;
use solana_program::pubkey::Pubkey;
use std::collections::HashMap;
use std::str::FromStr;

#[repr(C)]
pub struct UpdatePriceInstruction {
    pub version: u32,
    pub cmd: i32,
    pub status: PriceStatus,
    pub unused: u32,
    pub price: i64,
    pub conf: u64,
    pub pub_slot: u64,
}

pub struct ProductResult {
    pub key: Pubkey,
    pub price_accounts: [u8; 32],
}

pub struct PriceAccount {
    pub key: Pubkey,
    pub expo: i32,
    pub twap: i64,
}

pub trait PythAccount {
    fn is_valid(&self) -> bool;
}
impl PythAccount for Mapping {
    fn is_valid(&self) -> bool {
        if self.magic != MAGIC || self.atype != AccountType::Mapping as u32 || self.ver != VERSION_2
        {
            return false;
        }
        true
    }
}
impl PythAccount for Product {
    fn is_valid(&self) -> bool {
        if self.magic != MAGIC || self.atype != AccountType::Product as u32 || self.ver != VERSION_2
        {
            return false;
        }
        true
    }
}
trait PythProduct {
    fn get_symbol(&self) -> Option<String>;
    fn decode_attributes(&self) -> Option<HashMap<String, String>>;
}

impl PythProduct for Product {
    fn get_symbol(&self) -> Option<String> {
        let attr_map = match self.decode_attributes() {
            None => return None,
            Some(i) => i,
        };
        let k = String::from("symbol");
        match attr_map.get(&k) {
            Some(i) => return Some(i.clone()),
            None => return None,
        };
    }
    fn decode_attributes(&self) -> Option<HashMap<String, String>> {
        let mut attributes = HashMap::new();
        let mut pr_attr_sz = self.size as usize - PROD_HDR_SIZE;
        let mut pr_attr_it = (&self.attr[..]).iter();
        while pr_attr_sz > 0 {
            let key = get_attr_str(&mut pr_attr_it);
            let val = get_attr_str(&mut pr_attr_it);
            pr_attr_sz -= 2 + key.len() + val.len();
            // println!("{:.<16} {}", key, val);
            attributes.insert(key, val);
        }
        Some(attributes)
    }
}

impl PythAccount for Price {
    fn is_valid(&self) -> bool {
        if self.magic != MAGIC || self.atype != AccountType::Price as u32 || self.ver != VERSION_2 {
            return false;
        }
        let _ = match &self.ptype {
            PriceType::Price => "price",
            _ => return false,
        };
        true
    }
}
impl PythAccount for UpdatePriceInstruction {
    fn is_valid(&self) -> bool {
        let _ = match &self.status {
            PriceStatus::Trading => "trading",
            _ => return false,
        };
        if self.price == 0 {
            return false;
        }
        true
    }
}

pub struct PythClient {
    pub client: RpcClient,
}
impl PythClient {
    pub fn new(url: &String) -> Result<PythClient, &'static str> {
        // url error handling
        return Ok(PythClient {
            client: RpcClient::new(url.to_string()),
        });
    }
    pub fn get_product_account(
        &self,
        map_key: &str,
        symbol: &str,
    ) -> Result<ProductResult, &'static str> {
        // read pyth_map_key account data and verify it is the correct account
        // mapping accounts stored as linked list so we iterate until empty
        let mut akey = Pubkey::from_str(&map_key).unwrap();

        loop {
            let map_data = match self.client.get_account_data(&akey) {
                Err(_) => return Err("not a valid pyth mapping account"),
                Ok(i) => i,
            };
            let map_acct = cast::<Mapping>(&map_data).unwrap();
            if !map_acct.is_valid() {
                return Err("not a valid pyth mapping account");
            }

            // loop over products until we find one that matches are symbol
            let mut i = 0;
            for prod_akey in &map_acct.products {
                let prod_pkey = Pubkey::new(&prod_akey.val);
                let prod_data = self.client.get_account_data(&prod_pkey).unwrap();
                let prod_acct = match cast::<Product>(&prod_data) {
                    Some(prod_acct) => prod_acct,
                    None => continue, // go to next loop if no product account
                };
                if !prod_acct.is_valid() {
                    continue;
                }

                // loop through reference attributes and find symbol
                let prod_attr_sym = match prod_acct.get_symbol() {
                    Some(s) => s,
                    None => continue,
                };
                if prod_attr_sym != symbol {
                    i += 1;
                    if i == map_acct.num {
                        break;
                    }
                    continue;
                }
                if !prod_acct.px_acc.is_valid() {
                    return Err("pyth price account in valid");
                }
                return Ok(ProductResult {
                    key: prod_pkey,
                    price_accounts: prod_acct.px_acc.val,
                });
            }
            // go to next Mapping account in list
            if !map_acct.next.is_valid() {
                break;
            }
            akey = Pubkey::new(&map_acct.next.val);
        }
        println!(
            "See {} for a list of symbols",
            "https://pyth.network/markets/"
        );
        return Err("product account not found");
    }
    pub fn get_price_account(&self, px_acct: [u8; 32]) -> Result<PriceAccount, &'static str> {
        // check if price account is valid
        let mut price_pkey = Pubkey::new(&px_acct);
        let mut p: &Price;
        loop {
            let price_data = match self.client.get_account_data(&price_pkey) {
                Ok(price_acct) => price_acct,
                Err(_) => return Err("error getting price data"), // go to next loop if no product account
            };
            p = cast::<Price>(&price_data).unwrap();
            if p.is_valid() {
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
            price_pkey = Pubkey::new(&p.next.val);
            continue;
        }
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

#[cfg(test)]
mod tests {
    use crate::pyth::{PythAccount, PythProduct, MAGIC};
    use pyth_client::{AccKey, AccountType, Product, VERSION_2};
    #[cfg(test)]
    struct Setup {
        product: Product,
    }
    impl Setup {
        fn new() -> Self {
            Self {
                product: Product {
                    magic: MAGIC,
                    ver: VERSION_2,
                    atype: AccountType::Product as u32,
                    size: 165,
                    px_acc: AccKey {
                        val: [
                            202, 128, 186, 109, 195, 46, 8, 208, 111, 26, 168, 134, 1, 30, 237, 29,
                            119, 199, 123, 233, 235, 118, 28, 193, 13, 114, 183, 208, 162, 253, 87,
                            166,
                        ],
                    },
                    attr: [
                        6, 115, 121, 109, 98, 111, 108, 7, 69, 84, 72, 47, 85, 83, 68, 10, 97, 115,
                        115, 101, 116, 95, 116, 121, 112, 101, 6, 67, 114, 121, 112, 116, 111, 7,
                        99, 111, 117, 110, 116, 114, 121, 2, 85, 83, 14, 113, 117, 111, 116, 101,
                        95, 99, 117, 114, 114, 101, 110, 99, 121, 3, 85, 83, 68, 5, 116, 101, 110,
                        111, 114, 4, 83, 112, 111, 116, 11, 100, 101, 115, 99, 114, 105, 112, 116,
                        105, 111, 110, 8, 69, 116, 104, 101, 114, 101, 117, 109, 14, 103, 101, 110,
                        101, 114, 105, 99, 95, 115, 121, 109, 98, 111, 108, 6, 69, 84, 72, 85, 83,
                        68, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                    ],
                },
            }
        }
    }

    #[test]
    fn invalid_product_acct() {
        let mut s = Setup::new();
        assert_eq!(s.product.is_valid(), true);
        s.product.magic = s.product.magic - 1;
        assert_eq!(s.product.is_valid(), false);
    }
    #[test]
    fn valid_product_symbol() {
        let symbol = Setup::new().product.get_symbol().unwrap();
        let expected_symbol = String::from("ETH/USD");
        assert_eq!(symbol, expected_symbol);
    }
}
