# Music Presence API

A simple Cloudflare Workers API that stores and serves album artwork.

## Usage

1. Create a Cloudflare KV namespace called "artwork"
  - Run `pnpm wrangler kv namespace create artwork`
2. Update `wrangler.jsonc` with the namespace's ID
3. Run `pnpm run deploy`
