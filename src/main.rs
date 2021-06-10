use clap::{App, Arg};
use solana_client::{
  rpc_client::RpcClient
};
use solana_program::{
  pubkey::Pubkey
};
use std::{
  str::FromStr
};

use pyth_client::{
  AccountType,
  Mapping,
  Product,
  Price,
//   PriceType,
//   PriceStatus,
//   CorpAction,
  cast,
  MAGIC,
  VERSION_1,
  PROD_HDR_SIZE
};

fn main() {
    let matches = App::new("Pyth-TWAP")
        .version("0.1.0")
        .author("Conner <ConnerNGallagher@gmail.com>")
        .about("using pyth price oracle to calculate twap")
        .arg(Arg::with_name("symbol")
            .help("the symbol to calculate the TWAP for (BTC/USD)")
            .index(1)
            .required(true))
        // .arg(Arg::with_name("interval")
        //     .help("the interval to calculate the TWAP over in hours")
        //     .index(2)
        //     .default_value("24")
        //     .required(false))
        .arg(Arg::with_name("local")
            .short("l")
            .help("run on a local instance of solana (http://localhost:8899)"))
        .arg(Arg::with_name("pyth")
            .short("p")
            .help("sets the public key of the pyth mapping account")
            .takes_value(true)
            .default_value("ArppEFcsybCLE8CRtQJLQ9tLv2peGmQoKWFuiUWm4KBP")
            .required(false))
        .arg(Arg::with_name("interval")
            .short("i")
            .help("the interval to calculate the TWAP over in hours")
            .takes_value(true)
            .default_value("24")
            .required(false))
        .get_matches();

    let symbol = matches.value_of("symbol").unwrap();
    println!("Value for symbol: {}", symbol );

    let interval = matches.value_of("interval").unwrap();
    println!("Value for interval: {}", interval );

    let pyth_map_key = matches.value_of("pyth").unwrap();
    println!("using new pyth map key: {}", pyth_map_key);

    let mut url = "http://api.devnet.solana.com";
    if matches.is_present("local") {
        url = "http://localhost";
    }

    println!("Connecting to Solana @: {}", url );
    let rpc_client = RpcClient::new( url.to_string() );
    
    // read pyth_map_key account data and verify it is the correct account
    let akey = Pubkey::from_str( pyth_map_key ).unwrap();
    let map_data = rpc_client.get_account_data( &akey ).unwrap();
    let map_acct = cast::<Mapping>( &map_data );
    assert_eq!( map_acct.magic, MAGIC, "not a valid pyth account" );
    assert_eq!( map_acct.atype, AccountType::Mapping as u32,
                "not a valid pyth mapping account" );
    assert_eq!( map_acct.ver, VERSION_1,
                "unexpected pyth mapping account version" );
    

    // loop over products until we find one that matches are symbol
    let mut i = 0;
    for prod_akey in &map_acct.products {
      let prod_pkey = Pubkey::new( &prod_akey.val );
      let prod_data = rpc_client.get_account_data( &prod_pkey ).unwrap();
      let prod_acct = cast::<Product>( &prod_data );
      assert_eq!( prod_acct.magic, MAGIC, "not a valid pyth account" );
      assert_eq!( prod_acct.atype, AccountType::Product as u32,
                  "not a valid pyth product account" );
      assert_eq!( prod_acct.ver, VERSION_1,
                  "unexpected pyth product account version" );

      // loop through reference attributes and find symbol
      println!( "product_account .. {:?}", prod_pkey );
      let pr_attr_sym = get_attr_symbol(prod_acct);
      if pr_attr_sym != symbol {
          continue;
      }

      println!("looping through price accounts");
      // print all Prices that correspond to this Product
      if prod_acct.px_acc.is_valid() {
        let px_pkey = Pubkey::new( &prod_acct.px_acc.val );
        loop {
          let pd = rpc_client.get_account_data( &px_pkey ).unwrap();
          let pa = cast::<Price>( &pd );
          assert_eq!( pa.magic, MAGIC, "not a valid pyth account" );
          assert_eq!( pa.atype, AccountType::Price as u32,
                     "not a valid pyth price account" );
          assert_eq!( pa.ver, VERSION_1,
                      "unexpected pyth price account version" );
          println!( "  price_account .. {:?}", px_pkey );
        //   println!( "    price_type ... {}", get_price_type(&pa.ptype));
          println!( "    exponent ..... {}", pa.expo );
        //   println!( "    status ....... {}", get_status(&pa.agg.status));
        //   println!( "    corp_act ..... {}", get_corp_act(&pa.agg.corp_act));
          println!( "    price ........ {}", pa.agg.price );
          println!( "    conf ......... {}", pa.agg.conf );
          println!( "    valid_slot ... {}", pa.valid_slot );
          println!( "    publish_slot . {}", pa.agg.pub_slot );
          return
        }
      }
      // go to next product
      i += 1;
      if i == map_acct.num {
        break;
      }
    }

    // read transaction data for last <interval> hours to build price history


    
}

fn get_attr_str<'a,T>( ite: & mut T ) -> String
where T : Iterator<Item=& 'a u8>
{
  let mut len = *ite.next().unwrap() as usize;
  let mut val = String::with_capacity( len );
  while len > 0 {
    val.push( *ite.next().unwrap() as char );
    len -= 1;
  }
  return val
}

// loops through a products reference data (key/val) and
// returns the value for symbol
fn get_attr_symbol(prod_acct: &Product) -> String {
  let mut pr_attr_sz = prod_acct.size as usize - PROD_HDR_SIZE;
  let mut pr_attr_it = (&prod_acct.attr[..]).iter();
  while pr_attr_sz > 0 {
    let key = get_attr_str( &mut pr_attr_it );
    let val = get_attr_str( &mut pr_attr_it );
    println!( "  {:.<16} {}", key, val );
    if key == "symbol" {
        return val.to_string();
    }
    pr_attr_sz -= 2 + key.len() + val.len();
  }
  return "".to_string();
}
 