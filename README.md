# A SIP Stack written in Rust

**WIP** This is a work in progress and is not yet ready for production use.

A RFC 3261 compliant SIP stack written in Rust. The goal of this project is to provide a high-performance, reliable, and easy-to-use SIP stack that can be used in various scenarios.


## TODO
- [ ] Transport support
  - [x] UDP
  - [ ] TCP
  - [ ] TLS
  - [ ] WebSocket
- [x] Digest Authentication
- [x] Transaction Layer
- [x] Dialog Layer
- [ ] WASM target

## Use Cases

This SIP stack can be used in various scenarios, including but not limited to:

- Integration with WebRTC for browser-based communication, such as WebRTC SBC.
- Building custom SIP proxies or registrars
- Building custom SIP user agents (SIP.js alternative)

## Why Rust?

We are a group of developers who are passionate about SIP and Rust. We believe that Rust is a great language for building high-performance network applications, and we want to bring the power of Rust to the SIP/WebRTC/SFU world.

## How to run

```bash
# the sip phone will serve at: YOUR_NETWORK_IP:25060
cargo run --example client
```

Make a call to `sip:YOUR_NETWORK_IP:25060` from another sip client.(e.g. [linphone](https://www.linphone.org/))
# Run with OpenAI realtime voice model

1. apply for OpenAI API key
2. set the API key in the environment variable `OPENAI_API_KEY`

```bash
export OPENAI_API_KEY=your_openai_api_key
# or moddify the .env file

cargo run --example client -- --realtime --prompt "What is the meaning of life?"

# or prompt from the text
cargo run --example client -- --realtime --prompt prompt.txt

```

### Windows

There are three requirements for building on Windows:

 * You must use a version of Rust which uses the MSVC toolchain
 * You must have [WinPcap](https://www.winpcap.org/) or [npcap](https://nmap.org/npcap/) installed
   (tested with version WinPcap 4.1.3) (If using npcap, make sure to install with the "Install Npcap in WinPcap API-compatible Mode")
 * You must place `Packet.lib` from the [WinPcap Developers pack](https://www.winpcap.org/devel.htm)
   in a directory named `lib`, in the root of this repository. Alternatively, you can use any of the
   locations listed in the `%LIB%`/`$Env:LIB` environment variables. For the 64 bit toolchain it is
   in `WpdPack/Lib/x64/Packet.lib`, for the 32 bit toolchain, it is in `WpdPack/Lib/Packet.lib`.