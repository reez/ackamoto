# ACKamoto

Bitcoin Core ACK tracker at [ackamoto.com](https://ackamoto.com)

## Local Development

```bash
cargo run
open index.html
```

## How It Works

- Fetches recent Bitcoin Core PRs and comments
- Scans for ACK types (ACK, Concept ACK, utACK, etc.)
- Generates static HTML page
- Updates automatically every 2 hours via GitHub Actions