# Music Presence

A Windows & MacOS app that sets your Discord presence to whatever music you're currently listening to, based on what your player reports to the operating system.

## Usage

Download the [latest release](https://github.com/appellation/music-rpc/releases) and install.

## Manual Build

1. Install JS dependencies with `pnpm install`
2. Create a Discord app in [the developer portal](https://discord.com/developers/applications)
   - Set `http://localhost` as a redirect URI in the OAuth2 tab
3. Setup the API, as described in [/api/README.md](/api/README.md)
4. Create a `.env` file in the project root
   - `CLIENT_ID`: the ID of your Discord app
   - `API_URL`: the base URL of the Cloudflare Worker you created in step 2
5. `pnpm tauri build`
6. Install the resulting binary
7. Enjoy!

### Notes

This requires an API only to serve artwork to the Discord media proxy for display in Discord clients. This app uploads the artwork when media changes, and the artwork is set to expire when the track ends.
