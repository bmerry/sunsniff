# Inverter telemetry capture

This program collects data from a Sunsynk/Deye router and makes it available
for use. It can collect the data in two ways (referred to as "frontends"):

1. By running on a router sitting between an inverter with an
   Inteless WiFi dongle and the remote server. In this mode it is a completely
   passive observer, so it cannot interface with the inverter's operation. This is
   called the `pcap` frontend.

2. By connecting a serial cable to the inverter, it is possible to query it
   interactively. This requires additional hardware, but allows the query
   interval be set (and made much faster than the 5 minute interval the dongle
   uses), and the dongle can be removed for better privacy and security. In this
   mode commands are sent to your inverter, but they only read (not write) the
   registers, so it is still pretty safe. This is the `modbus` frontend. See
   [this guide](https://kellerza.github.io/sunsynk/guide/deployment-options) for
   information on how to wire the RS485 cable. There are reports that the RS232
   connection works too.

There are also currently two "backends", which determine what to do with the
data.

1. Store the values in an Influxdb database (requires Influxdb2).
2. Broadcast the values over MQTT.

This is *alpha* software (although I am using it every day). All the schemas may
change. The data you collect might vanish, or leak onto the internet (but it's
already being sent unencrypted, which is why this project works in the first
place). The config file format may change. It may hang your router.

## Compilation

1. Install Rust e.g. using [these instructions](https://www.rust-lang.org/learn/get-started).
2. Ensure that you have a C compiler and linker, and libpcap installed.
3. Run `cargo install sunsniff` to install the binary. Alternatively,
   check out the repository and run `cargo build --release`. This will compile
   the binary to `target/release/sunsniff`.

If you want to cross-compile:

1. Install and set up [cross](https://github.com/cross-rs/cross) e.g. using
   [these
   instructions](https://github.com/cross-rs/cross/wiki/Getting-Started).
2. Run `cross build --release --target=armv7-unknown-linux-gnueabihf` (replace
   with your target architecture).
3. Find the binary in `target/<arch>/release/target`.

I had problems because the resulting binary needed a newer glibc than the host
I was targeting. To build a static binary, set the environment variable
`RUSTFLAGS` to `-C target-feature=+crt-static -lpcap`. I also found that DNS
wasn't working with glibc, so I ended up using a target of
`armv7-unknown-linux-musleabihf` instead.

## Configuration

Configuration is stored in a [TOML](https://toml.io/) file, which is passed on
the command line.

Configure one of the possible frontends (do not try to configure more
than one), and least one backend. It's possible to have more than one instance
of the same backend (the doubled square brackets are the TOML syntax that
allows for this).

### Pcap frontend

Create a `[pcap]` section. It has the following fields:

- `device` (required): the Ethernet device to capture. Note that the `any`
  device is not currently supported.
- `filter` (optional but recommended): A pcap filter to select the traffic to
  inspect. If the `device` handles data for any other devices on the network
  then setting `filter` is necessary to prevent other data from being
  accidentally interpreted as sensor readings.
- `file` (optional): if set to true, then `device` is interpreted as a pcap
  file rather than a device. Note that the pcap file is fully loaded into
  memory, so it should not be used with very large files.
- `timezone` (required): The timezone name used by the inverter. This is used
  to convert the timestamps to UTC.

I have the following setup:
```toml
[pcap]
device = "br0"
filter = "src host 192.168.0.21"
timezone = "Africa/Johannesburg"
```

### Modbus frontend

Create a `[modbus]` section. It has the following fields:

- `device` (required): the serial device, or the address for Modbus over TCP
  in the format host:port (the port is required even when using the Modbus
  default).
- `interval` (required): time (in seconds) between samples
- `baud` (optional): baud rate for the serial port. Defaults to 9600.
- `modbus_id` (optional): Modbus slave number of the inverter. Check your
  inverter settings. Defaults to 1.

I have the following configuration:

```toml
[modbus]
device = "/dev/ttyUSB0"
baud = 9600
interval = 20
```

### Influxdb2 backend

The readings are inserted into an Influxdb 2.x bucket. Note that the schema is
**not final**.

The configuration section looks like this:
```toml
[[influxdb2]]
host = "http://192.168.0.123:8086/"
org = "my_org"
bucket = "my_bucket"
token = "..."
```

The implementation tries very hard to deal with intermittent connections to
Influxdb, buffering messages until it is able to deliver them (but only in
memory; if the service is stopped, any pending messages are lost). Since the
updates are only sent every 5 minutes is can be quite practical to buffer
messages for hours or days, and I'm currently running the Influxdb server on
my home PC which is switched off at night.

The downside of this robustness is that if you get the configuration wrong,
the server won't stop with an error. It will just keep trying to deliver, and
use more and more memory to buffer the incoming messages.

### MQTT backend (Home Assistant)

This backend publishes sensor values to an MQTT broker. The topics are
specifically designed for use with [Home
Assistant](https://www.home-assistant.io/) and provide the appropriate
discovery information, but this doesn't prevent other use cases. You will need
to install an MQTT broker (Home Assistant supports Mosquitto as an add-on) and
configure Home Assistant to use it. A typical configuration then looks like
this:
```toml
[[mqtt]]
url = "mqtt://192.168.0.123:1883"
username = "my_username"
password = "my_password"
```
The username and password can be omitted if the broker doesn't require
authentication.

Unfortunately the MQTT library I'm using doesn't support MQTT
last will messages, so there is no availability information to indicate that
the service is running.

## Supported hardware

So far I've only tested this with my personal setup. I'm hoping other devices
will work too. If it works for you, please let me know. Note that it's unlikely
to work with the 3-phase inverters, as they use different registers.

- Inverter: Sunsynk 5 kW (Sunsynk-5K-SG01LP1)
- Dongle: unbranded Inteless dongle (it has red and green lights). Apparently
  the Sunsynk-branded dongle is the same thing.

## Troubleshooting

Logging is done with
[env_logger](https://docs.rs/env_logger/latest/env_logger/), so you can
enable debugging by setting the environment variable `RUST_LOG=debug`. There
isn't very much logging yet though.

TODO:
- Explain what to look for in a packet capture
- Explain that missing pcap filter can cause bogus data

## Changelog

### 0.4.1

Add additional sensors

- Power, voltage, current for a 3rd PV string
- Grid CT power (which is different from Grid power when Limit to Load
  is set)

### 0.4.0

- Fix handling of systems with two PV strings. Previously the PV Power metric
  reported only the power for the first string. There are now PV Power 1 and PV
  Power 2 metrics, and PV Power reports their sum.
- Support newer dongle firmware in the pcap backend, which uses a different
  packet size and layout.
- Update dependencies

### 0.3.2

- Update dependencies
- Use modbus-robust so that restarting mbusd will be handled robustly

### 0.3.1

- Update dependencies
- Fix cross builds for aarch64-linux-gnu

### 0.3

- Add `grid_connected` sensor
- Add sensors for programmed time blocks
- Make pcap frontend optional
- Fix cross compiling of pcap for some architectures

### 0.2

- Add modbus support.
- Rename `pv_voltage 1` to `pv_voltage_1` (the space was a typo).
- Change the offsets used for `grid_power`, `inverter_power`, `load_power`.
  This makes no difference on my inverter, but may give you more correct
  values if you have additional connections.
- Hopefully fix the kWh sensors to include the upper 32 bits, so that they
  can support values above 32767 kWh (untested).

### 0.1.2

- Attempt to connect to InfluxDB on startup and show a warning if it's not
  reachable.
- Bump versions of dependencies (this seems to fix InfluxDB with TLS).
- Improve documentation.

### 0.1.1

Add more fields.

### 0.1.0

First release.
