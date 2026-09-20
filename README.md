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

On each update, the checksum of the current order book state is computed and verified against checksum from the received message.

## Advanced usage
Supply multiple pairs
```
cargo r ETH/USD SOL/USD
```

Define order book depth (allowed values 10, 25, 100)
```
cargo r SOL/USD --depth 15
```

Specify retry policy
```
cargo r ETH/USD --retry-policy no-retry
```
