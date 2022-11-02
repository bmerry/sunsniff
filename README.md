# Inverter telemetry capture

This program is designed to run on a router sitting between an inverter with an
Inteless WiFi dongle and the remote server, intercept the sensor data, and make
it available for use.

This is *pre-alpha* software. All the schemas may change. The data you collect
might vanish, or leak onto the internet (but it's already being sent
unencrypted, which is why this project works in the first place). The config file
format may change. It may hang your router. While it supports authentication,
none of the interfaces have been tested with TLS.

The good news is that by design it intercepts traffic but does not send any
commands to your inverter, so it can't interfere with the inverter's
operation.

## Installation

TODO:
- write basic instructions for installation using cargo
- write instructions for cross-compilation

## Configuration

Configuration is stored in a [TOML](https://toml.io/) file, which is passed on
the command line. There is one mandatory section, `[pcap]`. There is one
mandatory setting, `device`, which is the Ethernet device to capture. Note
that the `any` device is not currently supported. The optional (but strongly
recommended) `filter` setting specifies a pcap filter to select the traffic to
inspect. I have the following setup:
```
[pcap]
device = "br0"
filter = "src host 192.168.0.201"
```

If the `device` handles data for any other devices on the network then setting
`filter` is necessary to prevent other data from being accidentally
interpreted as sensor readings and inserting bogus data.

For debugging, you can instead use a pcap file rather than a device. Note that
the pcap file is fully loaded into memory, so it should not be used with very
large files. An example looks like this:
```
[pcap]
device = "my-capture-file.pcap"
file = true
```

This just configures the data capture, but you should also specify at least
one backend to actually do something with it. For each backend type you can
configure multiple instances. This is why the section names are in double
brackets.

### Influxdb2 backend

The readings are inserted into an Influxdb bucket. Note that the schema is
**not final**.

The configuration section looks like this:
```
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
```
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
will work too. If it works for you, please let me know.

- Inverter: Sunsynk 5 kW (Sunsynk-5K-SG01LP1)
- Dongle: unbranded Inteless dongle (it has red and green lights)

## Troubleshooting

TODO:
- Explain how to enable logging
- Explain what to look for in a packet capture
- Explain that missing pcap filter can cause bogus data
