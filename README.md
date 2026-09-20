Order book websocket streaming
---

Toy project that implements live order book processing service, written in Rust.

## Running
```
# run
cargo r ETH/USD

# test
cargo t
```

## Design
`OrderBook` is an independent object that can `apply_update`s.
`KrakenFeed` is a particular implementation of an order book feed that uses `OrderBook`.

