# Pyth-TWAP

Pyth-TWAP is a rust application to calculate the Time Weighted Average Price (TWAP) using Solana's Pyth oracle. 
## Usage
Pyth-TWAP takes in a symbol (BTC/USD) and an optional interval in minutes (default is 60m). Pyth-TWAP can also be supplied an optional pyth mapping key.
| Arguement | Required  | Description |
| --- | --- | --- |
| symbol | Y  | The Pyth symbol to calculate the TWAP for. See https://pyth.network/markets |
| interval | N | The interval to calculate the TWAP over in minutes. Default value is 60. |
| Pyth mapping key | N | Public key of the Pyth mapping account. Default value is BmA9Z6FjioHJPpjT39QazZyhDRUdZy2ezwx4GiDdE2u2 |
| local | N | Flag to run on a local Solana instance |
| debug | N | Flag to turn on verbose logging |

For more help run
```bash
pyth-twap --help
```
### Basic
This example will calulcate the TWAP for BTC/USD over a 60m interval.
```bash
pyth-twap BTC/USD
```
### Advanced
This example will calculate the TWAP for DOGE/USD over a 15m interval on a local Solana instance with debugging turned on.
```bash
pyth-twap DOGE/USD -i 15 -l -d
```
