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
`BookWriter` runs on a separate OS thread and saves order book snapshot to specified sink.

On each update, the checksum of the current order book state is computed and verified against checksum from the received message.

## Advanced usage
Define order book depth (allowed values 10, 25, 100)
```
cargo r SOL/USD --depth 25
```

Specify retry policy
```
cargo r ETH/USD --retry-policy no-retry
```

## TODOs
- DuckDbSink?
- support choosing order book layout for storage - wide (current) vs long
- buffer parquet writes instead of allocating `Vec` on every snapshot
- think how to propagate errors from sink thread; add failing sink test
- replay fixture test; record real order book stream to jsonl, replay in a test and assert final state and written files
- property based testing (`proptest` crate), e.g. bids/asks order, ob depth, quantities never negative
- sink contract tests; given SinkHarness (which creates csv and parquet sinks) test that they behave the same: write order preserving, harness.read_back() from saved snapshots equal to the initial snapshots
