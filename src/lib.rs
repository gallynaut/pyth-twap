use pyth_client::{PriceType, Product, PROD_HDR_SIZE};

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
