use clap::Parser;
use etherparse::SlicedPacket;
use futures::channel::mpsc::UnboundedSender;
use futures::prelude::*;
use futures::stream::FuturesUnordered;
use futures::try_join;
use pcap::{Capture, Device, Packet, PacketCodec};
use serde::Deserialize;
use std::path::PathBuf;
use std::sync::Arc;

use sunsniff::influxdb2::Influxdb2Receiver;
use sunsniff::mqtt::MqttReceiver;
use sunsniff::receiver::{Field, Receiver, Update};

#[derive(Debug, Parser)]
#[clap(author, version)]
struct Args {
    #[clap()]
    config_file: PathBuf,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PcapConfig {
    device: String,
    #[serde(default)]
    file: bool,
    filter: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    pcap: PcapConfig,
    #[serde(default)]
    influxdb2: Vec<sunsniff::influxdb2::Config>,
    #[serde(default)]
    mqtt: Vec<sunsniff::mqtt::Config>,
}

const MAGIC_LENGTH: usize = 292;
const MAGIC_HEADER: u8 = 0xa5;
const FIELDS: &[Field] = &[
    Field::energy(70, "Battery", "Total charge", "battery_charge_total"),
    Field::energy(74, "Battery", "Total discharge", "battery_discharge_total"),
    Field::energy(82, "Grid", "Total import", "grid_import_total"),
    Field::energy(88, "Grid", "Total export", "grid_export_total"),
    Field::frequency(84, "Grid", "grid_frequency"),
    Field::energy(96, "Load", "Total consumption", "load_consumption_total"),
    Field::temperature_name(106, "Inverter", "DC Temperature", "inverter_temperature_dc"),
    Field::temperature_name(108, "Inverter", "AC Temperature", "inverter_temperature_ac"),
    Field::energy(118, "PV", "Total production", "pv_production_total"),
    Field::voltage(176, "Grid", "grid_voltage"),
    Field::voltage(184, "Load", "load_voltage"),
    Field::power(216, "Grid", "grid_power"),
    Field::power(228, "Load", "load_power"),
    Field::temperature(240, "Battery", "battery_temperature"),
    Field::new(244, "Battery", "SOC", "battery_soc", 1.0, 0.0, "%"),
    Field::power(248, "PV", "pv_power"),
    Field::power(256, "Battery", "battery_power"),
    Field::current(258, "Battery", "battery_current"),
    Field::frequency(260, "Load", "load_frequency"),
];

struct Codec {}

impl PacketCodec for Codec {
    type Item = Option<Arc<Update<'static>>>;

    fn decode(&mut self, packet: Packet<'_>) -> Self::Item {
        if let Ok(sliced) = SlicedPacket::from_ethernet(packet.data) {
            if sliced.payload.len() == MAGIC_LENGTH && sliced.payload[0] == MAGIC_HEADER {
                let serial = std::str::from_utf8(&sliced.payload[11..21]).unwrap_or("unknown");
                let timestamp = (packet.header.ts.tv_sec as i64) * 1000000000i64
                    + (packet.header.ts.tv_usec as i64) * 1000i64;
                let mut values = vec![];
                for field in FIELDS.iter() {
                    let bytes = &sliced.payload[field.offset..field.offset + 2];
                    let bytes = <&[u8; 2]>::try_from(bytes).unwrap();
                    let value = i16::from_be_bytes(*bytes);
                    let value = (value as f64) * field.scale + field.bias;
                    values.push(value);
                }
                let update = Update::new(timestamp, serial, FIELDS, values);
                return Some(Arc::new(update));
            }
        }
        None
    }
}

async fn run<S: Stream<Item = Result<<Codec as PacketCodec>::Item, pcap::Error>> + Unpin>(
    stream: &mut S,
    sinks: &mut [UnboundedSender<Arc<Update<'static>>>],
) -> Result<(), Box<dyn std::error::Error>> {
    loop {
        match stream.next().await {
            Some(item) => match item? {
                Some(update) => {
                    for sink in sinks.iter_mut() {
                        sink.unbounded_send(Arc::clone(&update))?;
                    }
                }
                None => {}
            },
            None => {
                break;
            }
        }
    }
    for sink in sinks.iter_mut() {
        sink.close().await?; // TODO: do these in parallel?
    }
    Ok(())
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let config = std::fs::read_to_string(args.config_file)?;
    let config: Config = toml::from_str(&config)?;

    let mut receivers: Vec<Box<dyn Receiver>> = vec![];
    for backend in config.influxdb2.iter() {
        receivers.push(Box::new(Influxdb2Receiver::new(&backend)));
    }
    for backend in config.mqtt.iter() {
        receivers.push(Box::new(MqttReceiver::new(&backend)?));
    }

    let mut sinks = vec![];
    let futures = FuturesUnordered::new();
    for receiver in receivers.iter_mut() {
        let (sink, stream) = futures::channel::mpsc::unbounded();
        futures.push(receiver.run(stream));
        sinks.push(sink);
    }

    let base_filter = "tcp";
    let filter = match &config.pcap.filter {
        Some(expr) => format!("({}) and ({})", base_filter, expr),
        None => String::from(base_filter),
    };

    // TODO: better handling of errors from receivers
    if config.pcap.file {
        let mut cap = Capture::from_file(&config.pcap.device)?;
        cap.filter(filter.as_str(), true)?;
        cap.set_datalink(pcap::Linktype::ETHERNET)?;
        /* cap.stream doesn't work on files. This is a somewhat hacky
         * workaround: it's probably going to load all the packets into
         * the sinks at once before giving them a chance to run.
         */
        let mut stream = futures::stream::iter(cap.iter(Codec {}));
        try_join!(
            run(&mut stream, &mut sinks),
            futures.collect::<Vec<_>>().map(|x| Ok(x))
        )?;
    } else {
        let device = Device::from(config.pcap.device.as_str());
        let cap = Capture::from_device(device)?.immediate_mode(true).open()?;
        let mut cap = cap.setnonblock()?;
        cap.filter(filter.as_str(), true)?;
        cap.set_datalink(pcap::Linktype::ETHERNET)?;
        let mut stream = cap.stream(Codec {})?;
        try_join!(
            run(&mut stream, &mut sinks),
            futures.collect::<Vec<_>>().map(|x| Ok(x))
        )?;
    }
    Ok(())
}
