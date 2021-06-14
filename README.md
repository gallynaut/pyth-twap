# Pyth-TWAP

Pyth-TWAP is a rust application to calculate the Time Weighted Average Price (TWAP) using Solana's Pyth oracle. 
## Usage
Pyth-TWAP takes in a symbol (BTC/USD) and an optional interval in minutes (default is 15m). Pyth-TWAP can also be supplied an optional pyth mapping key.
### Basic
This example will calulcate the TWAP for BTC/USD over a 15m interval.
```bash
pyth-twap BTC/USD
```
### Advanced
This example will calculate the TWAP for DOGE/USD over a 60m interval on a local Solana instance with debugging turned on.
```bash
pyth-twap DOGE/USD -i 60 -l -d
```