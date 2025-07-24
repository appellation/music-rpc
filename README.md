# Music Presence

A Windows & MacOS app that sets your Discord presence to whatever music you're currently listening to, based on what your player reports to the operating system.

## Usage

- Create a `.env` file in the project root
  - `CLIENT_ID`: the ID of your Discord app
  - `CLIENT_SECRET`: the client secret of your Discord app (go to the "OAuth2" tab in the [developer portal](https://discord.com/developers/applications))
  - `NGROK_AUTH_TOKEN`: the auth token, provided on the [ngrok dashboard](https://dashboard.ngrok.com)
  - `NGROK_DOMAIN`: a valid ngrok domain associated with your account (you can get one for free on the ["Domains" tab](https://dashboard.ngrok.com/domains))
- `pnpm tauri build`
- Install the resulting binary
- Enjoy!

### Notes

1. This requires ngrok in order to serve album artwork, which must be publicly accessible to the Discord media proxy in order to be displayed; images cannot be directly specified in the presence payload.
2. I'd love to provide pre-built binaries, but it's not safe since this currently uses secrets embedded directly in the binary.

## Development Plans

- The UI could use some improvement and perhaps more information (e.g. currently connected Discord clients).
- Windows is broken since I've been mostly developing on Mac.
- If there's a potential solution to the secrets issue, I'd love to chat about it. Top of mind solutions:
  - Some kind of backend service, which would enable 0-config setup: just download and run
  - In-app configuration: I'm not sure that this would net a ton of convenience, since setting up a Discord app and ngrok account are the hardest parts of running this app
- Linux support? (I don't personally use it, so someone else would have to be motivated to do this.)
- I've only tested this with TIDAL, so other music players may break this.
